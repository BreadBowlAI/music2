# In short

Hostile test packs for the picture runner (CARD-64, CARD-65), from the review of track `pictures`
(G1, G2, G15, G16): `pictures_tests.rs` draws each one's pictures and checks that the runner stops
it, refuses it in words, and that no other pack's pictures wait for it. Not packs anyone installs.

| Pack | Its pictures |
|---|---|
| `evil` | reach for what the sandbox took away, a name of 20 MB, a huge BigInt, a catastrophic regex, a long sort, `Atomics.wait`, endless recursion, too much memory, an endless loop, a count kept between draws, words with control characters, words of 400 and 401 characters |
| `hang` | its module spins at load, for ever: the tests never describe it |
| `hijack` | its module replaces `__m2_picture` and `JSON.stringify` at load, to send shapes no check passed |
