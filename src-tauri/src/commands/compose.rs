use lettre::{
    message::{header::ContentType, Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use serde::{Deserialize, Serialize};

const GMAIL_SMTP_HOST: &str = "smtp.gmail.com";

#[derive(Debug, Deserialize)]
pub struct SendRequest {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendResult {
    pub ok: bool,
}

#[tauri::command]
pub async fn send_email(
    app: tauri::AppHandle,
    req: SendRequest,
) -> Result<SendResult, String> {
    let password = crate::commands::auth::load_password(&app, &req.from)?;

    let from_mailbox: Mailbox = req.from.parse().map_err(|e: lettre::address::AddressError| e.to_string())?;

    let mut builder = Message::builder()
        .from(from_mailbox.clone())
        .subject(&req.subject);

    for addr in &req.to {
        let mb: Mailbox = addr.parse().map_err(|e: lettre::address::AddressError| e.to_string())?;
        builder = builder.to(mb);
    }

    for addr in &req.cc {
        let mb: Mailbox = addr.parse().map_err(|e: lettre::address::AddressError| e.to_string())?;
        builder = builder.cc(mb);
    }

    if let Some(ref reply_to) = req.in_reply_to {
        builder = builder.in_reply_to(reply_to.clone());
    }

    if let Some(ref refs) = req.references {
        builder = builder.references(refs.clone());
    }

    let email = builder
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(req.body.clone()),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(format!(
                            "<div style=\"font-family:sans-serif;font-size:14px;\">{}</div>",
                            req.body.replace('\n', "<br>")
                        )),
                ),
        )
        .map_err(|e| e.to_string())?;

    let creds = Credentials::new(req.from.clone(), password);

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(GMAIL_SMTP_HOST)
        .map_err(|e| e.to_string())?
        .credentials(creds)
        .build();

    mailer.send(email).await.map_err(|e| e.to_string())?;

    Ok(SendResult { ok: true })
}
