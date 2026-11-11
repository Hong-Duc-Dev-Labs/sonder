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
