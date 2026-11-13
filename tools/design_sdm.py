#!/usr/bin/env python3
"""Sigma-delta modulator table tool for Sonder.

Sonder's PCM→DSD path uses single-bit CIFF modulators. Their feedforward (`a`) and resonator
(`g`) tables are the published Schreier-synthesised single-bit sets for these exact bit rates
(see the references in `crates/sonder-dsp/src/sdm.rs`), except DSD64 5th order, which is
derived from the DSD128/DSD256 sets as the module docs there describe (same audio-band NTF
zeros, extrapolated denominator); this script keeps them reviewable and
answers the questions the firmware needs:

* is the loop stable (are all poles of the noise transfer function inside the unit circle)?
* how much out-of-band gain does it have (`H_inf`, Lee's criterion)?
* how much in-band noise does the shaping leave (integrated |NTF|^2 over 20 kHz)?

It also prints the tables in the Rust form so a changed set can be dropped into `sdm.rs`.

The topology is the one in `Sdm::step`:

    d[0] = s[0] - g[0]·s[1] + x - y[n-1]
    d[i] = s[i] + s[i-1] - g[i]·s[i+1]
    v    = x + Σ a[i]·d[i]
    y[n] = sign(v)

so the noise transfer function is

    NTF(z) = 1 / (1 + z^-1 · Σ a[i] · (z^-1 / (1 - z^-1))^i)

and its poles are the roots of

    D(w) = (1-w)^N + Σ a[i] · w^(i+1) · (1-w)^(N-i),  w = z^-1.

Pure standard library: run `python tools/design_sdm.py`, or `--rust` to print the tables.
"""

from __future__ import annotations

import argparse
import cmath
import math

RATES = {"DSD64": 2_822_400, "DSD128": 5_644_800, "DSD256": 11_289_600}
BAND_HZ = 20_000.0

TABLES = {
    ("DSD64", 5): {
        "a": [0.809008307688490, 0.314888846641351, 0.0721231740641844,
              0.00961758957091257, 0.000516717217197248],
        "g": [0.0, 6.98490595355071e-04, 0.0, 1.97734345702526e-03],
    },
    ("DSD128", 5): {
        "a": [0.808016362125685, 0.316129639744972, 0.0734835047943110,
              0.0100377576971692, 0.000620309683440734],
        "g": [0.0, 1.74653153894942e-04, 0.0, 4.94580504383930e-04],
    },
    ("DSD256", 5): {
        "a": [0.807768375734983, 0.316440095967511, 0.0738231738259889,
              0.0101432044963374, 0.000646658652275506],
        "g": [0.0, 4.36651951230006e-05, 0.0, 1.23660417994961e-04],
    },
    ("DSD64", 7): {
        "a": [0.784599817960974, 0.299634346028983, 0.0713049276218066,
              0.0113334107086916, 0.00114497642818158, 7.33018502803093e-05,
              6.80633400002018e-07],
        "g": [0.0, 3.96825873999969e-04, 0.0, 1.32436089566069e-03, 0.0,
              2.16898568341885e-03],
    },
    ("DSD128", 7): {
        "a": [0.783148010097334, 0.301437231238902, 0.0731891646224574,
              0.0120314098366875, 0.00132077937193861, 9.11181979169687e-05,
              2.59895240306562e-06],
        "g": [0.0, 9.92163123766340e-05, 0.0, 3.31199917300393e-04, 0.0,
              5.42540771343282e-04],
    },
    ("DSD256", 7): {
        "a": [0.782785077952658, 0.301888671316811, 0.0736594376027782,
              0.0122068270909817, 0.00136572694403914, 9.57806082134936e-05,
              3.13239368043838e-06],
        "g": [0.0, 2.48046933669715e-05, 0.0, 8.28068362972358e-05, 0.0,
              1.35653594733585e-04],
    },
}


def poly_mul(left: list[complex], right: list[complex]) -> list[complex]:
    out = [0j] * (len(left) + len(right) - 1)
    for i, a in enumerate(left):
        for j, b in enumerate(right):
            out[i + j] += a * b
    return out


def pow_one_minus_w(power: int) -> list[complex]:
    out = [1 + 0j]
    for _ in range(power):
        out = poly_mul(out, [1, -1])
    return out


def denominator(a: list[float], order: int) -> list[complex]:
    """D(w) = (1-w)^N + Σ a[i] w^(i+1) (1-w)^(N-i), ascending powers of w."""
    d = pow_one_minus_w(order)
    for i in range(1, order + 1):
        term = [0j] * (i + 1) + pow_one_minus_w(order - i)
        if len(term) < len(d):
            term += [0j] * (len(d) - len(term))
        for k, value in enumerate(term):
            d[k] += a[i - 1] * value
    return d


def durand_kerner(coeffs: list[complex], iterations: int = 500) -> list[complex]:
    """All roots of a monic polynomial, ascending powers of w."""
    degree = len(coeffs) - 1
    lead = coeffs[-1]
    normalised = [value / lead for value in coeffs[:-1]]
    radius = 1 + max((abs(value) for value in normalised), default=0.0)
    roots = [radius * cmath.exp(2j * math.pi * k / degree) for k in range(degree)]
    for _ in range(iterations):
        moved = False
        for i in range(degree):
            value = roots[i] ** degree
            for k in range(degree - 1, -1, -1):
                value += normalised[k] * roots[i] ** k
            denom = 1 + 0j
            for j in range(degree):
                if i != j:
                    denom *= roots[i] - roots[j]
            if abs(denom) < 1e-18:
                continue
            step = value / denom
            if abs(step) > 1e-16:
                roots[i] -= step
                moved = True
        if not moved:
            break
    return roots


def ntf(a: list[float], order: int, freq: float, fs: float) -> float:
    """|NTF| at `freq` (Hz)."""
    w = cmath.exp(-2j * math.pi * freq / fs)
    loop = 0 + 0j
    t = w / (1 - w)
    power = 1 + 0j
    for value in a:
        power *= t
        loop += value * power
    return 1.0 / abs(1 + w * loop)


def analyse(name: str, order: int) -> dict:
    table = TABLES[(name, order)]
    fs = RATES[name]
    coeffs = denominator(table["a"], order)
    roots = durand_kerner(coeffs)
    stable = max(abs(root) for root in roots) < 1.0
    hinf = max(ntf(table["a"], order, fs / 2.0 * k / 2000.0, fs) for k in range(1, 2001))
    # In-band noise power density, integrated over the audio band.
    steps = 400
    density = sum(
        ntf(table["a"], order, BAND_HZ * (k + 0.5) / steps, fs) ** 2 for k in range(steps)
    ) / steps
    integrated_db = 10.0 * math.log10(density * BAND_HZ)
    return {
        "rate": name,
        "order": order,
        "stable": stable,
        "max_pole": max(abs(root) for root in roots),
        "hinf": hinf,
        "inband_db": integrated_db,
    }


def print_rust() -> None:
    print("// Tables mirrored from tools/design_sdm.py")
    for (name, order), table in TABLES.items():
        a = ", ".join(f"{value:.16e}" for value in table["a"])
        g = ", ".join(f"{value:.16e}" for value in table["g"])
        print(f'// {name}, order {order}')
        print(f"a: &[{a}],")
        print(f"g: &[{g}],")
        print()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust", action="store_true", help="print the Rust tables")
    args = parser.parse_args()
    if args.rust:
        print_rust()
        return

    print(f"{'rate':<8}{'order':>6}{'stable':>8}{'max pole':>12}{'H_inf':>10}"
          f"{'in-band':>12}")
    ok = True
    for name in RATES:
        for order in (5, 7):
            row = analyse(name, order)
            ok &= row["stable"]
            print(f"{row['rate']:<8}{row['order']:>6}{str(row['stable']):>8}"
                  f"{row['max_pole']:>12.4f}{row['hinf']:>10.2f}"
                  f"{row['inband_db']:>10.1f} dB")
    print()
    print("stable: every pole of the noise transfer function is inside the unit circle")
    print("H_inf: worst out-of-band noise gain (Lee's criterion wants it low)")
    print("in-band: shaped noise power left in 0..20 kHz relative to flat quantisation")
    if not ok:
        raise SystemExit("unstable modulator table")


if __name__ == "__main__":
    main()
