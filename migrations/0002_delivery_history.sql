ALTER TABLE deliveries ADD COLUMN provider_name TEXT;
UPDATE deliveries d SET provider_name = p.name
FROM providers p WHERE d.provider_id = p.id;
ALTER TABLE deliveries ALTER COLUMN provider_name SET NOT NULL;
ALTER TABLE deliveries ALTER COLUMN provider_id DROP NOT NULL;
ALTER TABLE deliveries DROP CONSTRAINT deliveries_provider_id_fkey;
ALTER TABLE deliveries ADD CONSTRAINT deliveries_provider_id_fkey
    FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE SET NULL;
