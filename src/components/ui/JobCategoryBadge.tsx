const JOB_CATEGORY_STYLE: Record<string, { label: string; className: string }> = {
  confirmation: { label: "Confirmation", className: "bg-slate-100 text-slate-600 ring-slate-200" },
  rejection: { label: "Rejection", className: "bg-gray-100 text-gray-600 ring-gray-200" },
  assessment: { label: "Assessment", className: "bg-amber-50 text-amber-700 ring-amber-100" },
  screening: { label: "Recruiter / screening", className: "bg-sky-50 text-sky-700 ring-sky-100" },
  interview: { label: "Interview", className: "bg-violet-50 text-violet-700 ring-violet-100" },
  offer: { label: "Offer", className: "bg-emerald-50 text-emerald-700 ring-emerald-100" },
  other: { label: "Other", className: "bg-gray-100 text-gray-600 ring-gray-200" },
};

export default function JobCategoryBadge({
  isJobRelated,
  category,
}: {
  isJobRelated: boolean;
  category: string | null;
}) {
  if (!isJobRelated) return null;
  const badge = JOB_CATEGORY_STYLE[category ?? "other"] ?? JOB_CATEGORY_STYLE.other;
  return (
    <span className={`inline-flex normal-case tracking-normal rounded-full px-2 py-0.5 text-[10px] font-semibold ring-1 ${badge.className}`}>
      {badge.label}
    </span>
  );
}
