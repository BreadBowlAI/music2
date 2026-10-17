// The sound output and this machine's settings (lane C of the next track): what the status says of
// the output (gaps #13). Added to graph.ts's types from here, as graph8b.ts does.

declare module "./graph" {
  interface GraphStatus {
    /** The sound output's name; null with no player, or while none plays. */
    output?: string | null;
    /** No sound output started, so the song plays silently (gaps #13). */
    no_output?: boolean;
  }
}

export {};
