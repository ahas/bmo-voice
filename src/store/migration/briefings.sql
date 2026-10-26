-- The briefings the engine speaks (docs/store.md, 「voice_briefings」): each row names the synthesis that spoke it
-- and whether a listener plays it by itself.

ALTER TABLE "voice_briefings" ADD COLUMN IF NOT EXISTS "synthesis_id" bigint;
ALTER TABLE "voice_briefings" ADD COLUMN IF NOT EXISTS "plays" bigint NOT NULL DEFAULT 1;

CREATE INDEX IF NOT EXISTS "voice_briefings_synthesis" ON "voice_briefings" (synthesis_id);

-- A briefing taken in is numbered past every row kept before the engine spoke briefings.
SELECT setval(pg_get_serial_sequence('"voice_briefings"', 'run_id'),
  GREATEST(COALESCE((SELECT max("run_id") FROM "voice_briefings"), 0), 1));
