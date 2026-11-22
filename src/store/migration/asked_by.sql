-- Who a caller said asks a line (docs/store.md, 「voice_syntheses」), handed on as its derivation slot's asker.

ALTER TABLE "voice_syntheses" ADD COLUMN IF NOT EXISTS "asked_by" text;
