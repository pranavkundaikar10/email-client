import type { RecommendedAction } from "./api";

export function recommendationLabel(action: RecommendedAction): string {
  switch (action) {
    case "keep": return "Keep in inbox";
    case "follow_up": return "Follow up";
    case "archive": return "Archive — low risk";
    case "delete": return "Delete — recommended";
    default: return "Needs your review";
  }
}

export function recommendationTone(action: RecommendedAction): string {
  switch (action) {
    case "keep": return "text-emerald-700";
    case "delete": return "text-red-600";
    case "archive": return "text-gray-600";
    default: return "text-amber-700";
  }
}
