# @m2/vocals

**In short**
- music2's tools for a sung voice, as a pack that ships with music2 and installs into your store when it starts.
- **Tune** moves each sung note onto the nearest note of the song's key. Glide 0 jumps at once (Robot, the stepped pop sound); a little glide keeps the singer's vibrato (Natural, where it opens). Amount says how much of each note's error is fixed.
- With no key set, every note is allowed: set the song's key in the top bar.

```ts
import { tune } from "@m2/vocals";
```

Its sound code is its own kernels, `@m2/vocals:pitch`, `snap` and `shift`, in `kernels/vocals.wasm`.
Tune makes the song play 15 ms later, so every track stays in time with it. The guide's page
Audio says how to use it, under Tune a voice: `m2 docs audio` prints it.
