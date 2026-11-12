-- Up to four dominant vivid colours of a cover, most dominant first, as comma-separated
-- `#rrggbb`. Empty when the cover has none (greyscale); NULL until computed for covers stored
-- before this column existed.
ALTER TABLE art ADD COLUMN colors TEXT;
