# @test/spin

**In short:** a pack for music2's tests (CARD-85): two sounds whose kernels pass their sound
through while it is quiet, as a build's warm-up plays, and misbehave once it isn't. `spin` never
returns from a call; `slow` works about 6 ms at the start of each call. Each draws its curve with
`p.response()`, so a picture of it measures it.
