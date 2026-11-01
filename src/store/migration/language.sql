-- The language each synthesis is said in (docs/store.md, 「voice_syntheses」), so a line a stopped process left is
-- taken up in it.

ALTER TABLE "voice_syntheses" ADD COLUMN IF NOT EXISTS "language" text;
