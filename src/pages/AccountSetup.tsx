import { useState } from "react";
import { api } from "../lib/api";

interface Props {
  onConnected: (email: string) => void;
}

export default function AccountSetup({ onConnected }: Props) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      await api.addAccount(email.trim(), password.trim());
      onConnected(email.trim());
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-white">
      <div className="w-full max-w-sm px-6">
        <h1 className="text-2xl font-semibold text-gray-900 mb-1">
          Connect your Gmail
        </h1>
        <p className="text-sm text-gray-500 mb-8">
          Uses an App Password — no data leaves your machine.
        </p>

        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-gray-600 uppercase tracking-wide">
              Gmail address
            </label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="you@gmail.com"
              required
              className="px-3 py-2 border border-gray-200 rounded-lg text-sm outline-none focus:border-gray-400 transition-colors"
            />
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-gray-600 uppercase tracking-wide">
              App Password
            </label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="xxxx xxxx xxxx xxxx"
              required
              className="px-3 py-2 border border-gray-200 rounded-lg text-sm outline-none focus:border-gray-400 transition-colors font-mono"
            />
            <a
              href="https://myaccount.google.com/apppasswords"
              target="_blank"
              rel="noreferrer"
              className="text-xs text-blue-500 hover:text-blue-600 mt-0.5"
            >
              Get an App Password →
            </a>
          </div>

          {error && (
            <p className="text-xs text-red-500 bg-red-50 px-3 py-2 rounded-lg">
              {error}
            </p>
          )}

          <button
            type="submit"
            disabled={loading || !email || !password}
            className="mt-2 px-4 py-2.5 bg-gray-900 text-white text-sm font-medium rounded-lg hover:bg-gray-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            {loading ? "Connecting…" : "Connect account"}
          </button>
        </form>
      </div>
    </div>
  );
}
