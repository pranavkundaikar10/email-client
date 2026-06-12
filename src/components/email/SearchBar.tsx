import { Search, X } from "lucide-react";

interface Props {
  value: string;
  onChange: (v: string) => void;
  onClear: () => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
}

export default function SearchBar({ value, onChange, onClear, inputRef }: Props) {
  return (
    <div className="flex items-center gap-2 px-3 py-2 border-b border-gray-100 bg-white">
      <Search size={13} className="text-gray-400 flex-shrink-0" />
      <input
        ref={inputRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="Search emails…"
        className="flex-1 text-sm outline-none placeholder:text-gray-400 bg-transparent"
      />
      {value && (
        <button onClick={onClear} className="text-gray-300 hover:text-gray-500">
          <X size={13} />
        </button>
      )}
    </div>
  );
}
