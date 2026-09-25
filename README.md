# Productive Email

Productive Email is an experimental, local-first desktop email client for Gmail. It is built with Tauri, Rust, React, SQLite, and Ollama.

It is designed for people who want to triage an inbox quickly: sync recent mail, review threads in a keyboard-friendly interface, and use a local LLM to identify emails that may need attention.

> This is an early-stage project, not a production-hardened email client. Use a test account or a low-risk Gmail account first, and review the security notes below before connecting a primary mailbox.

## What it does

- Connects to Gmail over IMAP with a Gmail App Password.
- Syncs inbox headers and fetches an email body when it needs to display or analyze it.
- Supports archive, delete, star, search, replies, split inboxes, and keyboard navigation.
- Runs local AI email triage through Ollama. The default model is `gemma4:e4b`.
- Displays an AI summary, action items, importance score, severity, and deadline when available.
- Gradually analyzes at most one eligible email per sync cycle (once per minute), limited to unarchived Inbox mail received today.
- Includes a human-approved review queue: the model suggests context, but you choose whether to keep, follow up on, or archive a message.

## Requirements

- A current Node.js LTS release and npm.
- Rust stable and the native build tools required by Tauri for your operating system. Follow the [official Tauri prerequisite guide](https://v2.tauri.app/start/prerequisites/).
- [Ollama](https://ollama.com/) running locally for AI analysis.
- A Gmail account with IMAP access and a Gmail App Password.

The current credential storage implementation uses Unix file-permission APIs, so macOS and Linux are the supported development platforms. Windows support needs credential-storage work before it should be considered supported.

## Quick start

```bash
git clone https://github.com/pranavkundaikar10/email-client.git
cd email-client
npm install
ollama pull gemma4:e4b
npm run tauri dev
```

The last command starts the Vite development server and the Tauri desktop application. Ollama should remain running locally; the app contacts it at `http://localhost:11434`.

`gemma4:e4b` is a comparatively large local download. If it is too slow for your machine, you can change `DEFAULT_MODEL` in `src-tauri/src/commands/agent.rs` to a smaller Ollama model such as `qwen2.5:7b-instruct`, then pull that model with Ollama.

## Connect a Gmail account

This app currently uses Gmail IMAP plus an App Password; it does not support OAuth / “Sign in with Google.”

1. Enable 2-Step Verification on the Gmail account.
2. Create a Google App Password from [Google Account → App Passwords](https://myaccount.google.com/apppasswords).
3. Start the app, enter your Gmail address, and paste the generated App Password into the connection screen.
4. Wait for the initial inbox sync to finish.

Google requires 2-Step Verification before App Passwords can be created. Some work, school, Advanced Protection, or security-key-only accounts may not offer App Passwords. See [Google’s App Password documentation](https://support.google.com/accounts/answer/185833) for the current requirements.

Never commit an App Password, credentials file, or local database to Git.

## AI triage and review queue

The AI feature sends the sender, subject, and email body to the Ollama server running on your own machine. It asks the model to return structured JSON with a category, summary, action items, importance score (1–5), and an optional deadline.

Background analysis is deliberately rate-limited: it processes one new eligible email at a time after each inbox sync. You can also use **Analyze** on an open email to prioritize that specific thread.

The **Review** view is approval-first:

- **Keep** and **Follow up** save a local review decision only. They do not change Gmail. Follow up is currently a label, not a reminder or snooze feature.
- **Archive** is an explicit remote action. It removes the message from the Gmail Inbox (equivalent to Gmail Archive) and keeps it in All Mail; it does not delete the message.
- Opening an email currently marks it read locally and synchronizes Gmail’s `\Seen` flag.

The model can be wrong. Treat its score, recommendation, and extracted deadline as triage assistance—not as a substitute for reading important email.

## Data and security notes

- Mail metadata and fetched message bodies are stored in a local SQLite database in the app’s data directory.
- Gmail App Passwords are stored locally in `credentials.json` with owner-only (`0600`) permissions on Unix. They are **not yet stored in the operating system keychain**.
- The app communicates with Gmail for syncing, body retrieval, sending, and explicit mailbox actions such as archive/delete.
- The AI feature uses a local Ollama endpoint by default. Do not change its endpoint to a remote service unless you are comfortable sending email content to that service.
- External or inline email images may not render correctly yet. The app does not currently resolve all `cid:` inline-image references.

## Useful commands

```bash
# Run the desktop application in development
npm run tauri dev

# Build the frontend
npm run build

# Check the Rust application
cd src-tauri && cargo check

# Create a desktop bundle
npm run tauri build
```

## Current limitations

- Gmail only; IMAP settings are not configurable.
- App Password authentication only; no OAuth.
- No calendar integration, reminders, or automatic actions.
- Review decisions are local to this app and are not synced back to Gmail.
- The project does not yet include a license file. Add an explicit license before publishing or accepting contributions.
