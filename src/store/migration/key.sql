-- The caller's key for a line (docs/store.md, 「voice_syntheses」): no two rows that have not failed hold one key, so
-- two calls with one key at once take in one synthesis between them.

ALTER TABLE "voice_syntheses" ADD COLUMN IF NOT EXISTS "key" text;
CREATE UNIQUE INDEX IF NOT EXISTS "voice_syntheses_live_key" ON "voice_syntheses" ("key")
  WHERE "key" IS NOT NULL AND "state" <> 'failed';
