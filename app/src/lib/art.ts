/** Cover thumbnail URL served by the app's `sonder-art` scheme. */
export type ArtSize = 300 | 1000;

// Windows webviews reach custom schemes as http://<scheme>.localhost.
const base = navigator.userAgent.includes("Windows")
  ? "http://sonder-art.localhost"
  : "sonder-art://localhost";

export function artUrl(hash: string | null | undefined, size: ArtSize): string | null {
  return hash ? `${base}/${hash}/${size}` : null;
}
