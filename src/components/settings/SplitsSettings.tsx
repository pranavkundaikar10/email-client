import { useState, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { X, Plus, Trash2, ChevronUp, ChevronDown, GripVertical, RefreshCw } from "lucide-react";
import { api, type SplitRule, type ThinkingSettings } from "../../lib/api";

const RULE_TYPES: { value: SplitRule["type"]; label: string; hasValue: boolean }[] = [
  { value: "is_newsletter",    label: "Is a newsletter / mailing list", hasValue: false },
  { value: "from_domain",      label: "Sender domain is",               hasValue: true  },
  { value: "from_email",       label: "Sender email is exactly",        hasValue: true  },
  { value: "from_pattern",     label: "Sender email contains",          hasValue: true  },
  { value: "subject_contains", label: "Subject contains",               hasValue: true  },
];

interface LocalSplit {
  id: string;
  name: string;
  rules: SplitRule[];
  isNew?: boolean;
}

function parseSplit(raw: { id: string; name: string; position: number; rules: string }): LocalSplit {
  return {
    id: raw.id,
    name: raw.name,
    rules: (() => { try { return JSON.parse(raw.rules); } catch { return []; } })(),
  };
}

function formatModelSize(bytes: number): string {
  if (!bytes) return "";
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

interface Props { onClose: () => void; }

type SettingsSection = "ai" | "inbox";

export default function SplitsSettings({ onClose }: Props) {
  const queryClient = useQueryClient();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [triagePreferences, setTriagePreferences] = useState<string | null>(null);
  const [thinkingSettings, setThinkingSettings] = useState<ThinkingSettings | null>(null);
  const [activeSection, setActiveSection] = useState<SettingsSection>("ai");

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const { data: rawSplits = [] } = useQuery({
    queryKey: ["splits"],
    queryFn: api.getSplits,
  });

  const { data: configuredModel } = useQuery({
    queryKey: ["ai_model"],
    queryFn: api.getAiModel,
  });

  const {
    data: savedTriagePreferences = "",
    isSuccess: triagePreferencesLoaded,
  } = useQuery({
    queryKey: ["triage_preferences"],
    queryFn: api.getTriagePreferences,
  });

  const {
    data: savedThinkingSettings = { manual: true, background: false },
    isSuccess: thinkingSettingsLoaded,
  } = useQuery({
    queryKey: ["ai_thinking_settings"],
    queryFn: api.getThinkingSettings,
  });

  const {
    data: ollamaModels = [],
    error: ollamaError,
    isFetching: fetchingModels,
    refetch: refetchModels,
  } = useQuery({
    queryKey: ["ollama_models"],
    queryFn: api.getOllamaModels,
  });

  useEffect(() => {
    if (configuredModel && selectedModel === null) setSelectedModel(configuredModel);
  }, [configuredModel, selectedModel]);

  useEffect(() => {
    // Do not treat React Query's initial fallback value as the stored value.
    // Otherwise the textarea is set to "" before SQLite responds and never
    // picks up a non-empty saved preference.
    if (triagePreferencesLoaded && triagePreferences === null) {
      setTriagePreferences(savedTriagePreferences);
    }
  }, [savedTriagePreferences, triagePreferences, triagePreferencesLoaded]);

  useEffect(() => {
    if (thinkingSettingsLoaded && thinkingSettings === null) {
      setThinkingSettings(savedThinkingSettings);
    }
  }, [savedThinkingSettings, thinkingSettings, thinkingSettingsLoaded]);

  const [splits, setSplits] = useState<LocalSplit[] | null>(null);
  const effective = splits ?? rawSplits.map(parseSplit);

  function setEffective(fn: (prev: LocalSplit[]) => LocalSplit[]) {
    setSplits(fn(effective));
  }

  // ── split-level actions ──────────────────────────────────────────────────

  function addSplit() {
    setEffective((prev) => [
      ...prev,
      { id: crypto.randomUUID(), name: "New Split", rules: [], isNew: true },
    ]);
  }

  function removeSplit(id: string) {
    setEffective((prev) => prev.filter((s) => s.id !== id));
  }

  function rename(id: string, name: string) {
    setEffective((prev) => prev.map((s) => (s.id === id ? { ...s, name } : s)));
  }

  function moveUp(idx: number) {
    if (idx === 0) return;
    setEffective((prev) => {
      const next = [...prev];
      [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
      return next;
    });
  }

  function moveDown(idx: number) {
    setEffective((prev) => {
      if (idx >= prev.length - 1) return prev;
      const next = [...prev];
      [next[idx], next[idx + 1]] = [next[idx + 1], next[idx]];
      return next;
    });
  }

  // ── rule-level actions ───────────────────────────────────────────────────

  function addRule(splitId: string) {
    setEffective((prev) =>
      prev.map((s) =>
        s.id === splitId
          ? { ...s, rules: [...s.rules, { type: "from_domain" }] }
          : s
      )
    );
  }

  function updateRule(splitId: string, rIdx: number, patch: Partial<SplitRule>) {
    setEffective((prev) =>
      prev.map((s) =>
        s.id === splitId
          ? {
              ...s,
              rules: s.rules.map((r, i) =>
                i === rIdx ? { ...r, ...patch } : r
              ),
            }
          : s
      )
    );
  }

  function removeRule(splitId: string, rIdx: number) {
    setEffective((prev) =>
      prev.map((s) =>
        s.id === splitId
          ? { ...s, rules: s.rules.filter((_, i) => i !== rIdx) }
          : s
      )
    );
  }

  // ── save ─────────────────────────────────────────────────────────────────

  async function save() {
    setSaving(true);
    setError(null);
    try {
      if (selectedModel && selectedModel !== configuredModel) {
        await api.setAiModel(selectedModel);
      }
      const normalizedPreferences = (triagePreferences ?? savedTriagePreferences).trim();
      if (normalizedPreferences !== savedTriagePreferences) {
        await api.setTriagePreferences(normalizedPreferences);
      }
      const effectiveThinkingSettings = thinkingSettings ?? savedThinkingSettings;
      if (
        effectiveThinkingSettings.manual !== savedThinkingSettings.manual ||
        effectiveThinkingSettings.background !== savedThinkingSettings.background
      ) {
        await api.setThinkingSettings(effectiveThinkingSettings);
      }
      const existing = new Set(rawSplits.map((s) => s.id));
      const kept = new Set(effective.map((s) => s.id));

      // Delete removed splits
      for (const raw of rawSplits) {
        if (!kept.has(raw.id)) await api.deleteSplit(raw.id);
      }

      // Create new / update existing
      const finalIds: string[] = [];
      for (const s of effective) {
        if (s.isNew || !existing.has(s.id)) {
          const created = await api.createSplit(s.name);
          await api.updateSplit(created.id, s.name, s.rules);
          finalIds.push(created.id);
        } else {
          await api.updateSplit(s.id, s.name, s.rules);
          finalIds.push(s.id);
        }
      }

      await api.reorderSplits(finalIds);
      await api.recategorizeThreads();

      queryClient.invalidateQueries({ queryKey: ["splits"] });
      queryClient.invalidateQueries({ queryKey: ["threads"] });
      queryClient.invalidateQueries({ queryKey: ["ai_model"] });
      queryClient.invalidateQueries({ queryKey: ["triage_preferences"] });
      queryClient.invalidateQueries({ queryKey: ["ai_thinking_settings"] });
      setSplits(null);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="flex h-[min(590px,76vh)] w-[min(840px,92vw)] flex-col overflow-hidden rounded-xl bg-white shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-100">
          <h2 className="text-sm font-semibold text-gray-900">Settings</h2>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-600">
            <X size={15} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1">
          <aside className="w-44 flex-shrink-0 border-r border-gray-100 bg-gray-50/60 px-3 py-4">
            <p className="px-2 pb-2 text-[10px] font-semibold uppercase tracking-wide text-gray-400">Settings</p>
            <nav className="space-y-1" aria-label="Settings sections">
              {([
                ["ai", "AI"],
                ["inbox", "Inbox organization"],
              ] as const).map(([section, label]) => (
                <button
                  key={section}
                  type="button"
                  onClick={() => setActiveSection(section)}
                  className={`w-full rounded-md px-2 py-1.5 text-left text-xs transition-colors ${
                    activeSection === section
                      ? "bg-white font-medium text-gray-900 shadow-sm ring-1 ring-gray-200"
                      : "text-gray-500 hover:bg-gray-100 hover:text-gray-800"
                  }`}
                >
                  {label}
                </button>
              ))}
            </nav>
          </aside>

          {/* Each section keeps its own focused workspace while shared state
              remains mounted in this dialog, so switching never discards edits. */}
          <div className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
          {activeSection === "ai" && <div className="space-y-6">
          <section>
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-[11px] font-semibold uppercase tracking-wide text-gray-400">AI</h3>
              <button
                onClick={() => refetchModels()}
                disabled={fetchingModels}
                title="Refresh installed Ollama models"
                className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700 disabled:opacity-40"
              >
                <RefreshCw size={13} className={fetchingModels ? "animate-spin" : ""} />
              </button>
            </div>
            <div className="mt-2 rounded-lg border border-gray-200 bg-white">
              <div className="flex items-center justify-between gap-4 px-3 py-3">
                <div className="min-w-0">
                  <p className="text-xs font-medium text-gray-700">Local AI model</p>
                  <p className="mt-0.5 text-[11px] text-gray-400">Used for future analyses</p>
                </div>
                {ollamaError ? (
                  <p className="max-w-56 text-right text-[11px] text-red-600">Could not reach Ollama</p>
                ) : (
                  <select
                    value={selectedModel ?? configuredModel ?? ""}
                    onChange={(e) => setSelectedModel(e.target.value)}
                    disabled={ollamaModels.length === 0}
                    className="max-w-64 rounded-md border border-gray-200 bg-gray-50 px-2.5 py-1.5 text-xs text-gray-700 outline-none focus:border-indigo-400 disabled:opacity-50"
                  >
                    {ollamaModels.length === 0 ? (
                      <option value="">No local models found</option>
                    ) : (
                      ollamaModels.map((model) => (
                        <option key={model.name} value={model.name}>
                          {model.name}{model.size ? ` — ${formatModelSize(model.size)}` : ""}
                        </option>
                      ))
                    )}
                  </select>
                )}
              </div>
            </div>
          </section>

          <section>
            <h3 className="text-[11px] font-semibold uppercase tracking-wide text-gray-400">Thinking mode</h3>
            <p className="mt-1 text-xs text-gray-500">
              Reasoning can improve difficult triage, but makes local analysis slower.
            </p>
            <div className="mt-2 divide-y divide-gray-100 rounded-lg border border-gray-200 bg-white">
              {([
                ["manual", "Manual analysis", "Used when you choose Analyze for an email."],
                ["background", "Review Queue", "Used for automatic Review Queue processing."],
              ] as const).map(([key, label, description]) => {
                const effectiveThinkingSettings = thinkingSettings ?? savedThinkingSettings;
                return <label key={key} className="flex cursor-pointer items-center justify-between gap-3 px-3 py-2.5">
                  <span>
                    <span className="block text-xs font-medium text-gray-700">{label}</span>
                    <span className="mt-0.5 block text-[11px] text-gray-400">{description}</span>
                  </span>
                  <input
                    type="checkbox"
                    checked={effectiveThinkingSettings[key]}
                    onChange={(event) => setThinkingSettings({ ...effectiveThinkingSettings, [key]: event.target.checked })}
                    className="h-4 w-4 accent-indigo-600"
                    aria-label={`${label} thinking mode`}
                  />
                </label>;
              })}
            </div>
          </section>

          <section>
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-[11px] font-semibold uppercase tracking-wide text-gray-400">AI triage preferences</h3>
              <button
                type="button"
                onClick={() => setTriagePreferences("")}
                disabled={!(triagePreferences ?? savedTriagePreferences)}
                className="rounded px-1.5 py-1 text-[11px] font-medium text-gray-500 hover:bg-gray-100 hover:text-gray-800 disabled:opacity-40"
              >
                Reset
              </button>
            </div>
            <p className="mt-1 text-xs text-gray-500">
              Tell the local assistant what matters to you. Applied only to new or manually re-analyzed emails.
            </p>
            <textarea
              value={triagePreferences ?? savedTriagePreferences}
              onChange={(event) => setTriagePreferences(event.target.value.slice(0, 1000))}
              maxLength={1000}
              rows={5}
              placeholder="Example: I’m targeting backend and platform engineering roles. Prioritize recruiter scheduling, assessments, interviews, visa questions, and deadlines."
              className="mt-2 w-full resize-y rounded-lg border border-gray-200 bg-white px-3 py-2.5 text-xs leading-relaxed text-gray-700 outline-none placeholder:text-gray-300 focus:border-indigo-400"
            />
            <div className="mt-1.5 flex items-start justify-between gap-3 text-[10px] text-gray-400">
              <span>Core JSON rules and attachment protection always remain enabled.</span>
              <span className="flex-shrink-0">{(triagePreferences ?? savedTriagePreferences).length}/1000</span>
            </div>
          </section>
          </div>}

          {activeSection === "inbox" && <div className="space-y-4">
          <div>
          <h3 className="text-[11px] font-semibold uppercase tracking-wide text-gray-400">Inbox organization</h3>
          <p className="mt-1 text-xs text-gray-500">
            Split inbox rules: emails are assigned to the first matching split. A split with no rules is a catch-all.
          </p>
          </div>

          {/* Splits list */}
          {effective.map((split, idx) => (
            <div key={split.id} className="border border-gray-200 rounded-lg overflow-hidden">
              {/* Split header */}
              <div className="flex items-center gap-2 px-3 py-2 bg-gray-50">
                <GripVertical size={13} className="text-gray-300 flex-shrink-0" />
                <input
                  value={split.name}
                  onChange={(e) => rename(split.id, e.target.value)}
                  className="flex-1 text-sm font-medium text-gray-800 bg-transparent outline-none"
                />
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => moveUp(idx)}
                    disabled={idx === 0}
                    className="p-1 text-gray-400 hover:text-gray-600 disabled:opacity-30"
                  >
                    <ChevronUp size={13} />
                  </button>
                  <button
                    onClick={() => moveDown(idx)}
                    disabled={idx === effective.length - 1}
                    className="p-1 text-gray-400 hover:text-gray-600 disabled:opacity-30"
                  >
                    <ChevronDown size={13} />
                  </button>
                  <button
                    onClick={() => removeSplit(split.id)}
                    className="p-1 text-gray-400 hover:text-red-500"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>

              {/* Rules */}
              <div className="px-3 py-2 space-y-2">
                {split.rules.length === 0 && (
                  <p className="text-xs text-gray-400 italic">
                    No rules — catches all remaining emails
                  </p>
                )}
                {split.rules.map((rule, rIdx) => {
                  const def = RULE_TYPES.find((t) => t.value === rule.type);
                  return (
                    <div key={rIdx} className="flex items-center gap-2">
                      <select
                        value={rule.type}
                        onChange={(e) =>
                          updateRule(split.id, rIdx, {
                            type: e.target.value as SplitRule["type"],
                            value: undefined,
                          })
                        }
                        className="text-xs border border-gray-200 rounded px-2 py-1 outline-none flex-shrink-0"
                      >
                        {RULE_TYPES.map((t) => (
                          <option key={t.value} value={t.value}>
                            {t.label}
                          </option>
                        ))}
                      </select>
                      {def?.hasValue && (
                        <input
                          value={rule.value ?? ""}
                          onChange={(e) =>
                            updateRule(split.id, rIdx, { value: e.target.value })
                          }
                          placeholder={
                            rule.type === "from_domain" ? "github.com" :
                            rule.type === "from_email"  ? "boss@company.com" :
                            rule.type === "from_pattern" ? "noreply|alerts" :
                            "keyword"
                          }
                          className="flex-1 text-xs border border-gray-200 rounded px-2 py-1 outline-none min-w-0"
                        />
                      )}
                      <button
                        onClick={() => removeRule(split.id, rIdx)}
                        className="text-gray-300 hover:text-red-400 flex-shrink-0"
                      >
                        <X size={12} />
                      </button>
                    </div>
                  );
                })}
                <button
                  onClick={() => addRule(split.id)}
                  className="flex items-center gap-1 text-xs text-indigo-500 hover:text-indigo-700 mt-1"
                >
                  <Plus size={11} />
                  Add rule
                </button>
              </div>
            </div>
          ))}

          <button
            onClick={addSplit}
            className="flex items-center gap-1.5 text-sm text-gray-500 hover:text-gray-800 py-1"
          >
            <Plus size={14} />
            Add split
          </button>
          </div>}
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t border-gray-100">
          {error ? (
            <p className="text-xs text-red-500 truncate">{error}</p>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="px-3 py-1.5 text-sm text-gray-500 hover:text-gray-700"
            >
              Cancel
            </button>
            <button
              onClick={save}
              disabled={saving}
              className="px-4 py-1.5 bg-gray-900 text-white text-sm rounded-lg hover:bg-gray-700 disabled:opacity-40"
            >
              {saving ? "Saving…" : "Save"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
