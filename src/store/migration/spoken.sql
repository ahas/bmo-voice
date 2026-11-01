-- The words the voice says of each synthesis (docs/store.md, 「voice_syntheses」), beside the written words.

ALTER TABLE "voice_syntheses" ADD COLUMN IF NOT EXISTS "spoken" text;
