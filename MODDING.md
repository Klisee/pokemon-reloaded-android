# Mods experimentales

Esta rama es un laboratorio. No toca la rama `traduccion-pt`.

## Mods

Cada mod vive en `game/extraido/mods/<id>/` y necesita `mod.ini`:

```ini
name=Mi mod
version=0.1
enabled=1
description=Prueba
```

- `data/` se superpone sobre `game/extraido/data/`. Antes de sobrescribir se guarda una copia en `mods/.backup/<id>/`.
- Si pones `enabled=0`, el motor restaura los archivos respaldados.
- `scripts/<NombreDeScript>.gml` sustituye el código GML de ese script. Tiene prioridad sobre los parches del motor.
- Los scripts se cargan antes de `init()`.
- En Android, el apartado visual está en **Ajustes → Mods**: lista los mods y permite activarlos/desactivarlos. Los cambios se aplican al reiniciar el juego.
- En PC, por ahora se activan/desactivan editando `mod.ini`.

## Menú in-game

- Si hay al menos un mod activo, el motor crea `mods/menu.txt` y `mods/active.flag`.
- El menú del título muestra una fila **Mods** (con las opciones recolocadas para no invadir el pie); al pulsarla se abre una pantalla propia a tamaño completo, como la de Opciones, con la lista de mods.
- En el menú in-game puedes hacer clic en **CRT** o **Game Boy** para activarlos/desactivarlos en caliente. Los demás mods se siguen activando desde Ajustes → Mods en Android o editando `mod.ini` en PC.
- Solo **Volver** cierra la lista; el resto del título queda bloqueado mientras está abierta.


## Mod CRT

Un mod puede activar el postproceso CRT con `effect=crt` en su `mod.ini`. Los valores son
opcionales; los que falten usan el valor por defecto. El efecto se aplica a todo el juego,
incluidos título, menús y mapa.

```ini
name=CRT
enabled=1
effect=crt

curvature=0.055   # curvatura de la pantalla
scanline=0.40     # intensidad de las líneas de barrido
scanline_count=360 # número de líneas sobre la altura
mask=0.60         # máscara RGB de fósforo
aberration=0.0035 # separación de color en los bordes
vignette=0.35     # oscurecimiento de las esquinas
blur=0.0          # suavizado del píxel
noise=0.014       # grano
brightness=1.18
saturation=1.08
glow=0.15         # brillo de las zonas claras
corner=0.07       # radio de las esquinas
```

`0` desactiva el apartado correspondiente; `1` es el máximo. Los cambios se aplican al reiniciar.


## Mod Game Boy

Activa el filtro de cuatro tonos con `effect=gb`:

```ini
name=Game Boy
enabled=1
effect=gb

strength=1.0   # mezcla con la imagen original
dither=0.25    # dithering ordenado, sobre todo en medios tonos
contrast=0.90
brightness=1.02
```

El filtro comparte el mismo pase que el CRT, así que ambos pueden estar activos a la vez.

Opcionalmente puede aplicar un audio lo-fi a toda la mezcla:

```ini
audio=gb
audio_bits=6
audio_rate=8000
audio_mono=1
```

## Probar

```bash
cd /home/klisee/pokemon-reloaded-experimental
cargo run -p gm8emulator --release -- "game/extraido/Proyecto Reloaded The Last Beta 1.9.2.2 Full.exe" -3
```
