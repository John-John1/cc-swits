import { Loader2, Square, Waypoints } from "lucide-react";
import type { ClaudeAppBridgeStatus } from "@/lib/api/claudeApp";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface ClaudeAppBridgeBannerProps {
  status?: ClaudeAppBridgeStatus;
  isLoading?: boolean;
  isStopping?: boolean;
  onStop: () => void;
}

export function ClaudeAppBridgeBanner({
  status,
  isLoading = false,
  isStopping = false,
  onStop,
}: ClaudeAppBridgeBannerProps) {
  const active = status?.running ?? false;
  const message = status?.message?.trim();
  const error = status?.lastError?.trim();
  const hint = status?.launchCommand?.trim();
  const providerName = status?.providerName?.trim();

  return (
    <div
      className={cn(
        "rounded-xl border px-4 py-3 transition-colors",
        active ? "border-emerald-500/30 bg-emerald-500/5" : "border-border bg-card",
      )}
    >
      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
        <div className="space-y-1.5">
          <div className="flex items-center gap-2 text-sm font-medium">
            {isLoading ? (
              <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
            ) : (
              <Waypoints
                className={cn(
                  "h-4 w-4",
                  active ? "text-emerald-500" : "text-muted-foreground",
                )}
              />
            )}
            <span>Claude App takeover</span>
            <span
              className={cn(
                "rounded-full px-2 py-0.5 text-xs",
                active ? "bg-emerald-500/10 text-emerald-600" : "bg-muted text-muted-foreground",
              )}
            >
              {active ? "Watching" : "Idle"}
            </span>
          </div>

          {message && <p className="text-sm text-muted-foreground">{message}</p>}
          {providerName && (
            <p className="text-xs text-muted-foreground">Selected provider: {providerName}</p>
          )}
          {hint && <p className="text-xs leading-5 text-muted-foreground">{hint}</p>}
          {error && <p className="text-xs leading-5 text-red-500">{error}</p>}
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onStop}
            disabled={!active || isStopping}
            className="gap-1.5"
          >
            {isStopping ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Square className="h-3.5 w-3.5" />
            )}
            Stop watching
          </Button>
        </div>
      </div>
    </div>
  );
}
