# @m2/vocals

**In short**
- music2's tools for a sung voice, as a pack that ships with music2 and installs into your store when it starts.
- **Tune** moves each sung note onto the nearest note of the song's key. Glide 0 jumps at once (Robot, the stepped pop sound); a little glide keeps the singer's vibrato (Natural, where it opens). Amount says how much of each note's error is fixed. Its card draws its pitch line as it plays: your voice and the note it moves it to.
- **Formant**, on Tune's card, changes the voice's colour and not its notes: down is deeper and darker, up smaller and brighter.
- With no key set, every note is allowed: set the song's key in the top bar.
- The **Voice changer** moves a voice's notes (Pitch) and its colour (Formant), each on its own: both down for a deep voice (its preset Deep), both up for a chipmunk (Chipmunk). It follows no key. Its card shows your voice's spectrum as it comes in and goes out.
- The **Vocoder** makes a synth talk: put it on the synth's track and pick the voice in its card's **Voice** row. The synth plays its notes, and the voice gives them its words. **Clarity** lets the voice's own S and T sounds through, **Smooth** is how fast it lets go of each sound, and **Formant** moves the voice's colour on the synth: down a deeper robot, up a brighter one. Until a voice is picked it plays the synth as it is. Its card shows the synth's spectrum as it comes in and as it goes out, talking.
- **A voice's notes**: right-click a recorded vocal clip, then **Find notes**. Each sung note shows in the Notes view, and you drag it to fix its pitch or its timing. Two kernels do it, which music2 runs, never song code: `notes` finds the notes, and `voice` plays the ones you edited.

```ts
import { tune, vocoder, voiceChanger } from "@m2/vocals";
```

Its sound code is its own kernels, `@m2/vocals:pitch`, `snap`, `shift`, `vocoder`, `notes` and `voice`, in `kernels/vocals.wasm`.
Tune and the Voice changer make the song play 15 ms later, so every track stays in time with them; the Vocoder adds no delay.
The guide's page Audio says how to use it, under Tune a voice, Change a voice, Make a synth talk and Edit a voice's notes:
`m2 docs audio` prints it.
