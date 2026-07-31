import { useCallback, useRef, useState } from "react";
import { api, asBrewError } from "../api/client";
import type { BrewError, OpEvent, OpRequest } from "../api/types";

export interface LogLine {
  key: number;
  text: string;
  tone: "out" | "err" | "phase";
}

export interface Operation {
  id: number | null;
  command: string;
  phase: string;
  percent: number | null;
  lines: LogLine[];
  running: boolean;
  /** Set when Homebrew asked something we have no terminal to answer. */
  blockedOn: string | null;
  error: BrewError | null;
  finishedAt: number | null;
  succeeded: boolean;
}

const IDLE: Operation = {
  id: null,
  command: "",
  phase: "",
  percent: null,
  lines: [],
  running: false,
  blockedOn: null,
  error: null,
  finishedAt: null,
  succeeded: false,
};

/** Keep the log bounded; a big upgrade can emit thousands of lines. */
const MAX_LINES = 700;

export function useOperations(onSettled?: () => void) {
  const [operation, setOperation] = useState<Operation>(IDLE);
  const lineKey = useRef(0);

  const append = useCallback((text: string, tone: LogLine["tone"]) => {
    setOperation((op) => {
      const lines = [...op.lines, { key: lineKey.current++, text, tone }];
      return { ...op, lines: lines.slice(-MAX_LINES) };
    });
  }, []);

  const handle = useCallback(
    (event: OpEvent) => {
      switch (event.event) {
        case "started":
          setOperation((op) => ({ ...op, id: event.data.id, command: event.data.command }));
          break;
        case "phase":
          setOperation((op) => ({ ...op, phase: event.data.label, percent: null }));
          append(event.data.label, "phase");
          break;
        case "progress":
          setOperation((op) => ({ ...op, percent: event.data.percent }));
          break;
        case "output":
          // Phase lines are already logged with their own styling.
          if (!event.data.text.startsWith("==>")) {
            append(event.data.text, event.data.origin === "stderr" ? "err" : "out");
          }
          break;
        case "needsInput":
          setOperation((op) => ({ ...op, blockedOn: event.data.text }));
          break;
        case "finished":
          setOperation((op) => ({
            ...op,
            running: false,
            percent: null,
            succeeded: event.data.success,
            finishedAt: Date.now(),
            phase: event.data.cancelled
              ? "Cancelled"
              : event.data.success
                ? "Done"
                : "Failed",
          }));
          break;
      }
    },
    [append],
  );

  const run = useCallback(
    async (request: OpRequest) => {
      setOperation({ ...IDLE, running: true, phase: "Starting" });
      lineKey.current = 0;
      try {
        await api.run(request, handle);
      } catch (e) {
        const error = asBrewError(e);
        setOperation((op) => ({
          ...op,
          running: false,
          error,
          finishedAt: Date.now(),
          phase: error.kind === "cancelled" ? "Cancelled" : "Failed",
        }));
      } finally {
        onSettled?.();
      }
    },
    [handle, onSettled],
  );

  const cancel = useCallback(async () => {
    setOperation((op) => {
      if (op.id !== null) void api.cancel(op.id).catch(() => undefined);
      return { ...op, phase: "Cancelling…" };
    });
  }, []);

  const dismiss = useCallback(() => setOperation(IDLE), []);

  return { operation, run, cancel, dismiss };
}
