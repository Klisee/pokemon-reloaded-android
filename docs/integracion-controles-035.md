# Integración de controles táctiles 0.3.5-touch en el port 0.3.6

**Base:** rama `main` del port 0.3.6 de Klisee, sin sobrescribir los cambios modernos.
**Fuente:** `pokemon-reloaded-android-0.3.5-touch-V-corregida.zip`, basado en `v0.3.5` (`d88580669426f357a5c782e51a5c3b834c6f7eb3`).

## Funciones integradas

- Botón **8** en Exploración: a la izquierda de PKM, configuración de posición y tamaño, etiqueta para tema clásico, sprite `alpha8.png`.
- **Mochila/Navegación:** joystick, Z y X disponibles; se conservan los gestos de las listas.
- **Caja del PC:** un toque breve sigue siendo clic izquierdo, mantener un dedo fijo durante 1 segundo provoca clic derecho; mover el dedo cancela el clic largo y permite arrastrar desde la celda original. Multitouch, cancelación y pérdida de foco sueltan/cancelan entrada.
- **Sprites modernos originales** de Z, X, C, V corregida, base y palanca del joystick. Conserva la personalización por usuario; originales editables de alta resolución están en `artwork-source/touch-update`.
- **Verificación:** `verification/touch_tests.rs` y `android/app/src/test/java/com/klisee/reloaded/TouchUpdateTest.kt`, más herramientas de preparación de sprites.
- **Firma:** si falta una clave release, la compilación release no usa la firma debug; el responsable del port puede firmarla con su propia clave.

## Preservación del proyecto

Se comparó el ZIP con la base oficial 0.3.5: 387 archivos coincidían, se importaron cambios puntuales y se conservaron los avances de 0.3.6 (motor Android, audio, Gradle, actualizaciones, etc.).
La versión sigue siendo 0.3.6; `versionCode` y la firma final deberán revisarse antes de una publicación oficial.

## Validación pendiente

**Esta integración no se ha compilado ni probado físicamente en Android en esta conversación.**
Se incluyen pruebas automatizadas para revisión; hace falta ejecutarlas, construir la APK con el SDK/NDK del proyecto y comprobar en el teléfono la mochila, el equipo, la caja del PC, la V, la persistencia de controles, la vibración y los gestos multitouch.
La APK oficial solo puede actualizarse con la firma que administra el creador.
