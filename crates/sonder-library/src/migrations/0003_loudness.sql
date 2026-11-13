-- Integrated loudness and sample peak measured by the library's background analysis for
-- tracks that carry no ReplayGain tags. album_lufs/album_peak describe the whole album (a
-- duration-weighted energy average of its measured tracks), so album levelling can move the
-- album without changing the relative loudness of its tracks.
ALTER TABLE tracks ADD COLUMN loudness_lufs REAL;
ALTER TABLE tracks ADD COLUMN loudness_peak REAL;
ALTER TABLE tracks ADD COLUMN album_lufs REAL;
ALTER TABLE tracks ADD COLUMN album_peak REAL;
