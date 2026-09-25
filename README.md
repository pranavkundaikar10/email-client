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

## Install prerequisites

The current credential-storage implementation uses Unix file-permission APIs, so macOS and Linux are the supported development platforms. Windows needs credential-storage work before it should be considered supported.

### macOS

Install the Xcode Command Line Tools, then install a current Node.js LTS release, Rust stable, and Ollama:

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh
# Make Cargo available in this terminal immediately after Rustup finishes
source "$HOME/.cargo/env"
```

Install Node.js LTS from [nodejs.org](https://nodejs.org/) and Ollama from [ollama.com](https://ollama.com/), then verify:

```bash
node --version
npm --version
cargo --version
ollama --version
```

### Linux

Install Node.js LTS, Rust stable, and Ollama, then install the distribution-specific WebKit, OpenSSL, compiler, and app-indicator packages required by Tauri. The exact package names vary by distribution; follow the [official Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

### No separate Tauri or database install

Do **not** need to install Tauri globally. `npm install` installs the project-pinned Tauri CLI from `package.json`, and `npm run tauri dev` uses that local copy.

SQLite is embedded in the Rust application through `sqlx`; there is no database server to install, configure, or start. The app creates its local database and runs migrations automatically on first launch.

You also need a Gmail account with IMAP access and a Gmail App Password.

## Run from a fresh clone

```bash
git clone https://github.com/pranavkundaikar10/email-client.git
cd email-client
npm install
ollama pull gemma4:e4b
npm run tauri dev
```

The last command is the required desktop-app command: it starts Vite **and** the Tauri window. Ollama should remain running locally; the app contacts it at `http://localhost:11434`.

Do not use `npm run dev` to launch the desktop app. That command starts only Vite, so opening `http://localhost:1420` in a normal browser is expected and does not launch Tauri.

`gemma4:e4b` is a comparatively large local download. If it is too slow for your machine, pull a smaller model such as `qwen2.5:7b-instruct`, then choose it under **Settings → Local AI model**. The selector lists models installed in Ollama dynamically and affects future analyses.

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

## Troubleshooting setup

| Symptom | Fix |
| --- | --- |
| A normal browser opens instead of the desktop app | Run `npm run tauri dev`, not `npm run dev`. |
| `tauri: command not found` | From the repository root, run `npm install`, then use `npm run tauri dev`. Do not run `tauri dev` directly unless you intentionally installed a global CLI. |
| `cargo` is not found after installing Rust | Run `source "$HOME/.cargo/env"` or open a new terminal, then retry. |
| A native compiler is missing | Install the Tauri system prerequisites for your operating system, then retry. |
| Ollama model error or no models in Settings | Start Ollama, run `ollama pull gemma4:e4b`, then refresh **Settings → Local AI model**. |
| Gmail login fails | Use a Google App Password, not the normal Gmail password, and confirm 2-Step Verification is enabled. |

## Current limitations

- Gmail only; IMAP settings are not configurable.
- App Password authentication only; no OAuth.
- No calendar integration, reminders, or automatic actions.
- Review decisions are local to this app and are not synced back to Gmail.
- The project does not yet include a license file. Add an explicit license before publishing or accepting contributions.
