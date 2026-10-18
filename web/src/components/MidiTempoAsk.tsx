import { useRef } from "react";
import { bpm, bringMidi, useMidiAsk, type TempoChoice } from "../lib/midiFiles";
import { useModalKeys } from "../lib/modal";
import { Button } from "./ui";

/**
 * The tempo question before a MIDI file's new tracks (studio's Q35, D34): "groove.mid is at 128
 * BPM. The song is at 120 BPM." [Cancel] [Use 128] [Keep 120]. Keep is the main button, so Return
 * keeps the song's tempo: the notes play at the song's speed, by beats. Use writes the file's tempo
 * from the bar the tracks start at. Esc, or a click outside, cancels: nothing is written. The keys
 * are every dialog's (`useModalKeys`).
 */
export default function MidiTempoAsk() {
  const ask = useMidiAsk((s) => s.ask);
  const close = () => useMidiAsk.setState({ ask: null });
  const answer = (tempo: TempoChoice) => {
    if (!ask) return;
    close();
    void bringMidi(ask.file, ask.to, tempo);
  };
  const box = useRef<HTMLDivElement>(null);
  useModalKeys(box, { onEscape: close, onDefault: () => answer("keep"), active: !!ask });
  if (!ask) return null;
  const [file, song] = [bpm(ask.fileBpm), bpm(ask.songBpm)];
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div ref={box} role="alertdialog" aria-modal aria-labelledby="midi-ask-title" data-testid="midi-ask" className="w-[440px] max-w-full p-6 rounded-md bg-raised shadow-pop animate-dialog cursor-default">
        <h2 id="midi-ask-title" className="text-lg font-semibold text-fg-strong">
          {`${ask.file.name} is at ${file} BPM. The song is at ${song} BPM.`}
        </h2>
        <p className="mt-2 text-base text-fg-muted">{`Keep ${song}: the notes play at your song's speed.`}</p>
        <div className="mt-6 flex justify-end gap-2">
          <Button variant="secondary" size="lg" data-testid="midi-cancel" onClick={close}>
            Cancel
          </Button>
          <Button variant="secondary" size="lg" data-testid="midi-use" onClick={() => answer("use")}>
            {`Use ${file}`}
          </Button>
          <Button variant="primary" size="lg" autoFocus data-testid="midi-keep" onClick={() => answer("keep")}>
            {`Keep ${song}`}
          </Button>
        </div>
      </div>
    </div>
  );
}
