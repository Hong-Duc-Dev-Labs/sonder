-- Public ("Anyone with the link") Google Drive source and track metadata.
-- Existing v8 Google Drive roots default to auth_mode = 'oauth', remote_kind = 'folder'.

ALTER TABLE roots ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'oauth';
ALTER TABLE roots ADD COLUMN remote_kind TEXT NOT NULL DEFAULT 'folder';

ALTER TABLE tracks ADD COLUMN remote_auth_mode TEXT;
