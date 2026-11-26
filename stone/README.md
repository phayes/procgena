# procgena_stone

Generate procedural rocks in Rust. `procgena_stone` carves a sphere into a rock shape,
roughens its surface with noise, and turns the result into a triangle mesh, on the
CPU or (planned) the GPU. The workspace also includes `procgena_stone_bevy`, a Bevy
integration.

This project is under construction. Its open follow-ups are in the repository's
[ROADMAP.md](../ROADMAP.md).

## Credits

`procgena_stone` is a Rust port of
[Unity-Procedural-Rock-Generation](https://github.com/przemyslawzaworski/Unity-Procedural-Rock-Generation)
by Przemyslaw Zaworski (MIT licence). That project builds on Inigo Quilez's
Shadertoy [ldSSzV](https://www.shadertoy.com/view/ldSSzV) and Paul Bourke's
[Polygonising a scalar field](http://paulbourke.net/geometry/polygonise/)
(marching cubes tables).
