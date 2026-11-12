-- Why a failed synthesis ended (docs/store.md, 「voice_syntheses」), written with its state in one write.

ALTER TABLE "voice_syntheses" ADD COLUMN IF NOT EXISTS "failure" text;
