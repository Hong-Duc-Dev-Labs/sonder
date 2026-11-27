-- Remote source metadata for Google Drive folders/files and tracks.
-- Existing local roots and tracks default to source_kind = 'local'.

ALTER TABLE roots ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'local';
ALTER TABLE roots ADD COLUMN remote_folder_id TEXT;
ALTER TABLE roots ADD COLUMN display_name TEXT;
ALTER TABLE roots ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'oauth';
ALTER TABLE roots ADD COLUMN remote_kind TEXT NOT NULL DEFAULT 'folder';

ALTER TABLE tracks ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'local';
ALTER TABLE tracks ADD COLUMN remote_file_id TEXT;
ALTER TABLE tracks ADD COLUMN remote_folder_id TEXT;
ALTER TABLE tracks ADD COLUMN remote_modified_time INTEGER;
ALTER TABLE tracks ADD COLUMN remote_size INTEGER;
ALTER TABLE tracks ADD COLUMN remote_auth_mode TEXT;

CREATE INDEX roots_source_kind ON roots (source_kind);
CREATE INDEX roots_remote_folder ON roots (remote_folder_id);
CREATE INDEX tracks_source_kind ON tracks (source_kind);
CREATE INDEX tracks_remote_file ON tracks (remote_file_id);
