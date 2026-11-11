//! Builders for small tagged audio files of every indexed format except ALAC (MP4 cannot be
//! produced without an encoder; lofty covers it).
#![allow(dead_code)]

use std::fs;
use std::io::Cursor;
use std::path::Path;

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};

pub const DSD64: u32 = 2_822_400;

/// Tag values for a fixture; `None` / empty leaves the field out.
#[derive(Default, Clone)]
pub struct Meta<'a> {
    pub title: Option<&'a str>,
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
    pub album_artist: Option<&'a str>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<&'a str>,
    pub track_gain: Option<&'a str>,
    pub album_gain: Option<&'a str>,
    pub cover: Option<Vec<u8>>,
}

fn build_tag(kind: TagType, m: &Meta) -> Tag {
    let mut tag = Tag::new(kind);
    if let Some(v) = m.title {
        tag.set_title(v.into());
    }
    if let Some(v) = m.artist {
        tag.set_artist(v.into());
    }
    if let Some(v) = m.album {
        tag.set_album(v.into());
    }
    if let Some(v) = m.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, v.into());
    }
    if let Some(v) = m.track {
        tag.set_track(v);
    }
    if let Some(v) = m.disc {
        tag.set_disk(v);
    }
    if let Some(v) = m.year {
        tag.insert_text(ItemKey::RecordingDate, v.into());
    }
    if let Some(v) = m.track_gain {
        tag.insert_text(ItemKey::ReplayGainTrackGain, v.into());
    }
    if let Some(v) = m.album_gain {
        tag.insert_text(ItemKey::ReplayGainAlbumGain, v.into());
    }
    if let Some(bytes) = &m.cover {
        tag.push_picture(
            Picture::unchecked(bytes.clone())
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Png)
                .build(),
        );
    }
    tag
}

fn save(path: &Path, kind: TagType, m: &Meta) {
    build_tag(kind, m)
        .save_to_path(path, WriteOptions::default())
        .unwrap_or_else(|e| panic!("tagging {}: {e}", path.display()));
}

fn id3_blob(m: &Meta) -> Vec<u8> {
    let mut blob = Vec::new();
    build_tag(TagType::Id3v2, m)
        .dump_to(&mut blob, WriteOptions::default())
        .unwrap();
    blob
}

fn mkdirs(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
}

pub fn png(width: u32, height: u32, shade: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| image::Rgb([x as u8, y as u8, shade]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// FLAC with a STREAMINFO block (no audio frames) and Vorbis comments.
pub fn flac(path: &Path, rate: u32, bits: u32, channels: u32, seconds: u64, m: &Meta) {
    mkdirs(path);
    let total = u64::from(rate) * seconds;
    let mut si = Vec::new();
    si.extend_from_slice(&4096u16.to_be_bytes());
    si.extend_from_slice(&4096u16.to_be_bytes());
    si.extend_from_slice(&[0; 6]); // frame sizes unknown
    // 20 bits rate, 3 bits channels-1, 5 bits bits-1, 36 bits total samples.
    let packed: u64 = (u64::from(rate) << 44)
        | (u64::from(channels - 1) << 41)
        | (u64::from(bits - 1) << 36)
        | total;
    si.extend_from_slice(&packed.to_be_bytes());
    si.extend_from_slice(&[0; 16]); // MD5
    let mut file = b"fLaC".to_vec();
    file.push(0x80); // last block, STREAMINFO
    file.extend_from_slice(&(si.len() as u32).to_be_bytes()[1..]);
    file.extend_from_slice(&si);
    fs::write(path, file).unwrap();
    save(path, TagType::VorbisComments, m);
}

pub fn wav(path: &Path, rate: u32, m: &Meta) {
    mkdirs(path);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(rate as i32 / 10) {
        w.write_sample((i % 300) as i16).unwrap();
        w.write_sample((-i % 300) as i16).unwrap();
    }
    w.finalize().unwrap();
    save(path, TagType::Id3v2, m);
}

/// 80-bit IEEE extended, as AIFF stores the sample rate.
fn extended(rate: u32) -> [u8; 10] {
    let e = 31 - rate.leading_zeros();
    let mut out = [0u8; 10];
    out[..2].copy_from_slice(&((16383 + e) as u16).to_be_bytes());
    out[2..].copy_from_slice(&((u64::from(rate)) << (63 - e)).to_be_bytes());
    out
}

pub fn aiff(path: &Path, rate: u32, bits: u16, m: &Meta) {
    mkdirs(path);
    let channels = 2u16;
    let frames = rate / 10;
    let mut comm = Vec::new();
    comm.extend_from_slice(&channels.to_be_bytes());
    comm.extend_from_slice(&frames.to_be_bytes());
    comm.extend_from_slice(&bits.to_be_bytes());
    comm.extend_from_slice(&extended(rate));
    let data_len = frames as usize * usize::from(channels) * usize::from(bits / 8);
    let mut body = b"AIFF".to_vec();
    body.extend_from_slice(b"COMM");
    body.extend_from_slice(&(comm.len() as u32).to_be_bytes());
    body.extend_from_slice(&comm);
    body.extend_from_slice(b"SSND");
    body.extend_from_slice(&((8 + data_len) as u32).to_be_bytes());
    body.extend_from_slice(&[0; 8]);
    body.resize(body.len() + data_len, 0);
    let mut file = b"FORM".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_be_bytes());
    file.extend_from_slice(&body);
    fs::write(path, file).unwrap();
    save(path, TagType::Id3v2, m);
}

/// DSF with an ID3v2 tag at the metadata pointer.
pub fn dsf(path: &Path, rate: u32, seconds: u64, m: &Meta) {
    mkdirs(path);
    let channels = 2u32;
    let block = 4096u32;
    let data_len = (block * channels) as u64;
    let id3 = id3_blob(m);
    let meta_ptr = 28 + 52 + 12 + data_len;
    let total = meta_ptr + id3.len() as u64;
    let mut f = Vec::new();
    f.extend_from_slice(b"DSD ");
    f.extend_from_slice(&28u64.to_le_bytes());
    f.extend_from_slice(&total.to_le_bytes());
    f.extend_from_slice(&meta_ptr.to_le_bytes());
    f.extend_from_slice(b"fmt ");
    f.extend_from_slice(&52u64.to_le_bytes());
    for v in [1u32, 0, 2, channels, rate, 1] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f.extend_from_slice(&(u64::from(rate) * seconds).to_le_bytes());
    f.extend_from_slice(&block.to_le_bytes());
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(b"data");
    f.extend_from_slice(&(12 + data_len).to_le_bytes());
    f.resize(f.len() + data_len as usize, 0x69);
    f.extend_from_slice(&id3);
    fs::write(path, f).unwrap();
}

fn dff_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u64).to_be_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

/// DSDIFF with uncompressed DSD, plus an `ID3 ` chunk when `m` has any value and a
/// `DIIN`/`DITI` title when `native_title` is given.
pub fn dff(path: &Path, rate: u32, millis: u64, m: Option<&Meta>, native_title: Option<&str>) {
    mkdirs(path);
    let channels = 2u16;
    let mut prop = b"SND ".to_vec();
    dff_chunk(&mut prop, b"FS  ", &rate.to_be_bytes());
    let mut chnl = channels.to_be_bytes().to_vec();
    chnl.extend_from_slice(b"SLFTSRGT");
    dff_chunk(&mut prop, b"CHNL", &chnl);
    let mut cmpr = b"DSD ".to_vec();
    cmpr.push(14);
    cmpr.extend_from_slice(b"not compressed");
    cmpr.push(0);
    dff_chunk(&mut prop, b"CMPR", &cmpr);

    let mut body = b"DSD ".to_vec();
    dff_chunk(&mut body, b"FVER", &0x0105_0000u32.to_be_bytes());
    dff_chunk(&mut body, b"PROP", &prop);
    let sound = (u64::from(rate) * millis / 1000) as usize * usize::from(channels) / 8;
    dff_chunk(&mut body, b"DSD ", &vec![0x69; sound]);
    if let Some(title) = native_title {
        let mut diti = (title.len() as u32).to_be_bytes().to_vec();
        diti.extend_from_slice(title.as_bytes());
        let mut diin = Vec::new();
        dff_chunk(&mut diin, b"DITI", &diti);
        dff_chunk(&mut body, b"DIIN", &diin);
    }
    if let Some(m) = m {
        dff_chunk(&mut body, b"ID3 ", &id3_blob(m));
    }
    let mut f = b"FRM8".to_vec();
    f.extend_from_slice(&(body.len() as u64).to_be_bytes());
    f.extend_from_slice(&body);
    fs::write(path, f).unwrap();
}

fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 4]; // version + flags
    b.extend_from_slice(body);
    mp4_box(kind, &b)
}

/// An ALAC `.m4a` with no audio samples whose sample entry carries a `free` padding box
/// after the `alac` config, as some encoders write it. lofty cannot name its codec.
pub fn alac_m4a_padded(path: &Path, rate: u32, bits: u8) {
    mkdirs(path);
    let channels = 2u16;
    // ALACSpecificConfig.
    let mut cookie = Vec::new();
    cookie.extend_from_slice(&4096u32.to_be_bytes()); // frame length
    cookie.extend_from_slice(&[0, bits, 40, 10, 14, channels as u8]);
    cookie.extend_from_slice(&255u16.to_be_bytes()); // max run
    cookie.extend_from_slice(&0u32.to_be_bytes()); // max frame bytes
    cookie.extend_from_slice(&0u32.to_be_bytes()); // average bit rate
    cookie.extend_from_slice(&rate.to_be_bytes());

    let mut entry = vec![0u8; 6];
    entry.extend_from_slice(&1u16.to_be_bytes()); // data reference index
    entry.extend_from_slice(&[0; 8]); // version, revision, vendor
    entry.extend_from_slice(&channels.to_be_bytes());
    entry.extend_from_slice(&u16::from(bits).to_be_bytes());
    entry.extend_from_slice(&[0; 4]); // compression id, packet size
    entry.extend_from_slice(&((rate.min(65_535)) << 16).to_be_bytes());
    entry.extend(full_box(b"alac", &cookie));
    entry.extend(mp4_box(b"free", &[0; 89]));

    let mut stsd = 1u32.to_be_bytes().to_vec();
    stsd.extend(mp4_box(b"alac", &entry));
    let empty_table = 0u32.to_be_bytes();
    let mut stsz = 0u32.to_be_bytes().to_vec(); // sample size
    stsz.extend_from_slice(&0u32.to_be_bytes()); // count
    let stbl = [
        full_box(b"stsd", &stsd),
        full_box(b"stts", &empty_table),
        full_box(b"stsc", &empty_table),
        full_box(b"stsz", &stsz),
        full_box(b"stco", &empty_table),
    ]
    .concat();
    let dinf = mp4_box(
        b"dinf",
        &full_box(
            b"dref",
            &[0, 0, 0, 1, 0, 0, 0, 12, b'u', b'r', b'l', b' ', 0, 0, 0, 1],
        ),
    );
    let minf = [full_box(b"smhd", &[0; 4]), dinf, mp4_box(b"stbl", &stbl)].concat();

    let mut mdhd = vec![0u8; 8]; // creation, modification
    mdhd.extend_from_slice(&rate.to_be_bytes()); // timescale
    mdhd.extend_from_slice(&0u32.to_be_bytes()); // duration
    mdhd.extend_from_slice(&[0x55, 0xc4, 0, 0]); // language "und", quality
    let mut hdlr = vec![0u8; 4];
    hdlr.extend_from_slice(b"soun");
    hdlr.extend_from_slice(&[0; 13]);
    let mdia = [
        full_box(b"mdhd", &mdhd),
        full_box(b"hdlr", &hdlr),
        mp4_box(b"minf", &minf),
    ]
    .concat();

    let mut tkhd = vec![0u8; 8];
    tkhd.extend_from_slice(&1u32.to_be_bytes()); // track id
    tkhd.extend_from_slice(&[0; 4 + 4 + 8 + 2 + 2 + 2 + 2]);
    for v in [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        tkhd.extend_from_slice(&v.to_be_bytes());
    }
    tkhd.extend_from_slice(&[0; 8]); // width, height
    let trak = mp4_box(
        b"trak",
        &[full_box(b"tkhd", &tkhd), mp4_box(b"mdia", &mdia)].concat(),
    );

    let mut mvhd = vec![0u8; 8];
    mvhd.extend_from_slice(&1000u32.to_be_bytes());
    mvhd.extend_from_slice(&0u32.to_be_bytes());
    mvhd.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate
    mvhd.extend_from_slice(&[0x01, 0x00, 0, 0]); // volume, reserved
    mvhd.extend_from_slice(&[0; 8]);
    for v in [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        mvhd.extend_from_slice(&v.to_be_bytes());
    }
    mvhd.extend_from_slice(&[0; 24]);
    mvhd.extend_from_slice(&2u32.to_be_bytes()); // next track id

    let mut ftyp = b"M4A ".to_vec();
    ftyp.extend_from_slice(&0u32.to_be_bytes());
    ftyp.extend_from_slice(b"M4A mp42isom");
    let file = [
        mp4_box(b"ftyp", &ftyp),
        mp4_box(b"moov", &[full_box(b"mvhd", &mvhd), trak].concat()),
        mp4_box(b"mdat", &[]),
    ]
    .concat();
    fs::write(path, file).unwrap();
}
