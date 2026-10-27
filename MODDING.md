# Mods y modo 3D experimental

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
- El menú in-game es de solo lectura por ahora; para activar/desactivar usa Ajustes → Mods en Android o edita `mod.ini` en PC.
- ESC/X cierra la lista.

## 3D

- Arranca con `-3` o `OPENGMK_VOXEL=1`.
- F7 alterna el modo en caliente.
- Ahora mismo es un prototipo: dibuja los tiles y las instancias como bloques de color con una cámara isométrica. No usa texturas, no clasifica paredes y no dibuja la interfaz del juego.
- El siguiente paso es añadir altura por tile, texturas y una cámara controlable.

## Probar

```bash
cd /home/klisee/pokemon-reloaded-experimental
cargo run -p gm8emulator --release -- "game/extraido/Proyecto Reloaded The Last Beta 1.9.2.2 Full.exe" -3
```
