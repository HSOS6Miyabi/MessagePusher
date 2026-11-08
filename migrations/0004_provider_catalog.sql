CREATE TABLE provider_catalog (
    kind TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    description TEXT NOT NULL,
    sort_order INTEGER NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT supported_provider_kind CHECK (
        kind IN ('telegram','wecom_app','wecom_webhook','email_smtp','email_resend')
    )
);
INSERT INTO provider_catalog(kind,label,description,sort_order,enabled) VALUES
    ('telegram','Telegram 机器人','通过 Telegram Bot 发送文字和附件，支持接收者订阅。',10,TRUE),
    ('wecom_app','企业微信自建应用','向企业内部成员推送文字和文件，支持成员授权。',20,TRUE),
    ('wecom_webhook','企业微信群机器人','通过企业微信群机器人向指定群聊推送消息和附件。',30,TRUE),
    ('email_smtp','SMTP 电子邮件','通过主程序的 SMTP 功能发送邮件及 MIME 附件。',40,TRUE),
    ('email_resend','Resend 邮件 API','通过 Resend 的 HTTPS API 发送邮件及附件。',50,TRUE);
