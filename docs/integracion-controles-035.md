# Integración de controles 0.3.5-touch en 0.3.6

**Rama de trabajo:** `integracion-controles-touch-035` (no modificar `main`).

## Ya incorporado

- Botón de tecla `8` configurable en el perfil de exploración, a la izquierda de PKM. Incluido en los catálogos de Android (`TouchLayouts.kt`) y Rust (`touch_controls.rs`).
- Botón `Z` en navegación/mochila, manteniendo `X` y joystick en ese contexto.
- Texto de la pantalla de configuración actualizado.

## Por integrar desde `pokemon-reloaded-android-0.3.5-touch-V-corregida.zip`

- Los **PNG originales**, en especial la `V` corregida, `Z`, `X`, `C`, `joystick-base`, `joystick-knob` y `alpha8`. Esta rama todavía **no contiene estos binarios**. Mientras tanto, el botón 8 utiliza el renderizador de texto existente.
- `src/OpenGMK/gm8emulator/src/box_touch.rs`: pulsación larga de un segundo en caja PC para clic derecho, con manejo de arrastre, multitouch, cancelación y pruebas.
- Integrar su uso en `android_main.rs` y la declaración del módulo, preservando los cambios de la versión 0.3.6 (audio, foco, empaquetado, etc.).
- Pruebas de compilación Android + Rust y validación en teléfono con el juego instalado.

No reemplazar en bloque los archivos `android_main.rs`, `GameActivity.kt`, `build.gradle.kts` ni otros modificados por Klisee después de 0.3.5; hacer merge selectivo.

**Estado:** parcial; no abrir Pull Request al repositorio original ni presentar APK como compilada antes de completar integración y pruebas.
