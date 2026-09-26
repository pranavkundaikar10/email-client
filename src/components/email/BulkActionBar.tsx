interface Props {
  count: number;
  onArchive: () => void;
  onDelete: () => void;
  onClear: () => void;
}

/** Shared bulk-selection controls for every thread-list view. */
export default function BulkActionBar({ count, onArchive, onDelete, onClear }: Props) {
  if (count === 0) return null;

  return (
    <div className="flex items-center gap-3 px-4 py-2 bg-indigo-50 border-b border-indigo-100 flex-shrink-0">
      <span className="text-xs text-indigo-700 font-medium flex-1">{count} selected</span>
      <button onClick={onArchive} className="text-xs text-gray-600 hover:text-gray-900 font-medium transition-colors">Archive</button>
      <button onClick={onDelete} className="text-xs text-red-500 hover:text-red-700 font-medium transition-colors">Delete</button>
      <button onClick={onClear} className="text-xs text-gray-400 hover:text-gray-600 transition-colors">Clear</button>
    </div>
  );
}
