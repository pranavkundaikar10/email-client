# Productive Email

Productive Email is an experimental, local-first desktop email client for Gmail. It is built with Tauri, Rust, React, SQLite, and Ollama.

It is designed for people who want to triage an inbox quickly: sync recent mail, review threads in a keyboard-friendly interface, and use a local LLM to identify emails that may need attention.

> This is an early-stage project, not a production-hardened email client. Use a test account or a low-risk Gmail account first, and review the security notes below before connecting a primary mailbox.

## What it does

- Connects to Gmail over IMAP using Google OAuth or a Gmail App Password.
- Keeps one lightweight IMAP IDLE connection open for near-real-time inbox changes, with a 60-second sync fallback; it fetches an email body only when it needs to display or analyze it.
- Supports archive, delete, star, search, replies, split inboxes, and keyboard navigation.
- Runs local AI email triage through Ollama. The default model is `gemma4:e4b`.
- Displays an AI summary, action items, importance score, severity, and deadline when available.
- Uses a durable Rust background worker to analyze eligible unarchived Inbox mail received today, one email at a time. It survives app restarts and never waits for the UI to remain focused.
- Includes a human-approved review queue: the model suggests context, but you choose whether to keep, follow up on, or archive a message.

## Install a release

Maintainers can create a tester build from GitHub: open **Actions → Build tester installers → Run workflow**, enter a new tag such as `v0.1.1-test.1`, and leave **Mark this as a tester prerelease** enabled. When all builds finish, share the GitHub Release link—not the temporary Actions artifact link—with testers.

### macOS

Download the DMG for your Mac from the repository's **Releases** page:

- **Apple Silicon** for M1, M2, M3, M4, and newer Macs.
- **Intel** for older Intel-based Macs.

Open the DMG and drag **Productive Email** to Applications. Early releases are unsigned, so macOS will require a one-time **Control-click → Open** confirmation on first launch. The app itself contains its database and all required runtime code: you do not need Node.js, Rust, Tauri, Xcode, or a terminal.

### Linux

Linux releases are built for 64-bit Intel/AMD computers. Download the format that fits your system:

- **`.AppImage`** for most Linux distributions. Mark it executable in its file properties, then open it.
- **`.deb`** for Ubuntu, Debian, and compatible distributions. Open it with the system package installer.

You still need [Ollama](https://ollama.com/) installed and running locally. After installing Ollama, run `ollama pull gemma4:e4b` in Terminal (or install another supported local model and select it in Settings).

## Development prerequisites

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

You also need a Gmail account with IMAP access. The app supports secure Google OAuth sign-in; a Gmail App Password remains available as a fallback.

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

Choose **Continue with Google** in the app to connect through the system browser. It uses OAuth with PKCE and stores the refresh token in the operating system credential store (macOS Keychain or the Linux Secret Service). The client ID is public by design; no client secret is embedded in the app.

For the current testing release, the Google Cloud project is in testing mode. The project owner must add each tester's Gmail address under **Google Auth Platform → Audience → Test users** before that tester can authorize the app. Google testing-mode refresh tokens expire after about seven days, so testers may need to sign in again during this phase.

For release builds, add `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` as **GitHub Actions secrets**. They are intentionally excluded from the repository. Local development reads the same values from the ignored `src-tauri/.env` file.

If Google OAuth is unavailable for your account, you can still use an App Password:

1. Enable 2-Step Verification on the Gmail account.
2. Create a Google App Password from [Google Account → App Passwords](https://myaccount.google.com/apppasswords).
3. Start the app, enter your Gmail address, and paste the generated App Password into the connection screen.
4. Wait for the initial inbox sync to finish.

Google requires 2-Step Verification before App Passwords can be created. Some work, school, Advanced Protection, or security-key-only accounts may not offer App Passwords. See [Google’s App Password documentation](https://support.google.com/accounts/answer/185833) for the current requirements.

Never commit an App Password, credentials file, or local database to Git.

## AI triage and review queue

The AI feature sends the sender, subject, and email body to the Ollama server running on your own machine. It asks the model to return structured JSON with a category, summary, action items, importance score (1–5), optional deadline, and optional calendar candidate.

Background analysis is deliberately rate-limited: Rust processes one eligible email at a time after sync/IMAP-IDLE wake-ups, using durable local jobs, retries, and a lease to prevent duplicate model calls. You can also use **Analyze** on an open email to prioritize that specific thread.

When an email contains a confirmed event with an explicit date, time, and timezone, the preview may offer a calendar candidate. It opens a prefilled Google Calendar page or downloads an `.ics` file; it never creates a remote calendar event automatically.

The **Review** view is approval-first:

- **Keep** saves a local review decision only. It does not change Gmail. **Follow up** schedules a local reminder in the Follow-ups view; it does not modify Gmail by itself.
- **Archive** is an explicit remote action. It removes the message from the Gmail Inbox (equivalent to Gmail Archive) and keeps it in All Mail; it does not delete the message.
- Opening an email currently marks it read locally and synchronizes Gmail’s `\Seen` flag.

The model can be wrong. Treat its score, recommendation, and extracted deadline as triage assistance—not as a substitute for reading important email.

## Data and security notes

- Mail metadata and fetched message bodies are stored in a local SQLite database in the app’s data directory.
- OAuth refresh tokens are stored in the operating system credential store; short-lived access tokens remain only in memory.
- Gmail App Passwords are stored locally in `credentials.json` with owner-only (`0600`) permissions on Unix. Prefer Google OAuth where possible.
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
| Gmail login fails | Prefer **Continue with Google** and make sure your address is in the project's test-user list. For the fallback, use a Google App Password—not the normal Gmail password—and confirm 2-Step Verification is enabled. |

## Current limitations

- Gmail only; IMAP settings are not configurable.
- Google OAuth currently supports the configured testing project only; public distribution will require completing Google's verification process for the Gmail scope.
- Calendar support is an early, opt-in export prototype: Google Calendar links and `.ics` downloads only. It does not create remote events.
- Background triage terminal-failure UI and manual retry controls are still being refined.
- Review decisions are local to this app and are not synced back to Gmail.
- The project does not yet include a license file. Add an explicit license before publishing or accepting contributions.
