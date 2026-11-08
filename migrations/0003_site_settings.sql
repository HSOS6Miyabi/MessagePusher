CREATE TABLE site_settings (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    site_name TEXT NOT NULL DEFAULT 'MessagePusher',
    site_description TEXT NOT NULL DEFAULT '一个接口，发送到你需要的每个平台。',
    announcement TEXT NOT NULL DEFAULT '',
    allow_registration BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO site_settings(id) VALUES(TRUE) ON CONFLICT DO NOTHING;
ALTER TABLE users ADD COLUMN session_version BIGINT NOT NULL DEFAULT 0;
