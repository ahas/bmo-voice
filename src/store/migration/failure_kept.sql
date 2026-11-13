-- Every failed synthesis holds a reason (docs/store.md, 「voice_syntheses」): one whose reason the engine never held
-- says so.

UPDATE "voice_syntheses" SET "failure" = 'its reason was not kept' WHERE "state" = 'failed' AND "failure" IS NULL;
