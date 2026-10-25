import { BalanceLens } from "./BalanceLens";
import { ClaudeCard } from "./ClaudeCard";
import { KeysPanel } from "./KeysPanel";

export function Home() {
  return (
    // On wide windows the page fits the window; the key list scrolls inside.
    <div className="mx-auto grid w-full max-w-[1000px] grid-cols-1 gap-4 px-6 pb-8 lg:min-h-0 lg:flex-1 lg:grid-cols-5 lg:grid-rows-[auto_minmax(9rem,1fr)]">
      <div className="lg:col-span-3">
        <BalanceLens />
      </div>
      <div className="lg:col-span-2">
        <ClaudeCard />
      </div>
      <div className="lg:col-span-5 lg:min-h-0">
        <KeysPanel />
      </div>
    </div>
  );
}
