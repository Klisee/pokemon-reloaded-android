# Propuesta de integración de controles táctiles — PR #88

Esta rama propone integrar controles táctiles adicionales y correcciones en Pokémon Reloaded Android. La integración **no altera la identidad ni la firma oficial de la aplicación**: la compilación y publicación final corresponden a Klisee.

## Funciones incluidas en esta rama

- **Botón 8** en Exploración, tanto para estilo clásico como moderno; incluye sprite, posición y tamaño personalizables.
- **Joystick** con respuesta a desplazamientos más pequeños del dedo.
- **Mochila y navegación:** joystick, Z y X disponibles; mantiene los gestos de las listas.
- **Caja del PC:** toque breve para seleccionar; el Pokémon levantado se mueve con el cursor libremente, sin un clic izquierdo continuo. El clic derecho de un segundo solo se aplica a una casilla válida cuando el Pokémon no está levantado y el dedo no se ha desplazado.
- **Datos, habilidades y movimientos:** se restauran controles para evitar quedarse sin forma de volver atrás.
- **Sprites modernos** de Z, X, C, V, base y palanca del joystick. Fuentes editables en `artwork-source/touch-update`.
- **Pruebas para revisión:** `verification/touch_tests.rs`, pruebas del comportamiento de la caja en Rust y `android/app/src/test/java/com/klisee/reloaded/TouchUpdateTest.kt`.

## Firma: reservada exclusivamente al creador

**No se incluye ninguna clave de firma privada, archivo JKS/keystore, certificado, contraseña ni APK firmada por el colaborador.**

- `.gitignore` evita incorporar los archivos locales de firma.
- `android/app/build.gradle.kts` usa `android/keystore.properties` **solo si Klisee lo proporciona localmente**. Si no está presente, `signingConfig = null` para el build `release`: no hay firma, y tampoco se sustituye por la firma de depuración.
- **Klisee debe usar la misma clave de firma oficial que empleó en las versiones anteriores**, mantener su `applicationId` y asignar el `versionCode`/`versionName` adecuados a la próxima release para que los usuarios puedan actualizar la aplicación normalmente. Una APK sin firmar no se puede instalar ni distribuir como actualización.
- No se necesita la clave original para revisar, editar o fusionar este código: **nunca debe compartirse en la PR**.

## Integración con el proyecto oficial

Los cambios son una propuesta de código fuente; no una APK alternativa para distribución. La rama oficial puede haber avanzado de versión y Klisee debe resolver cualquier diferencia de versión o conflicto con sus cambios recientes, manteniendo sus mejoras de Android y las partidas guardadas.

## Validación pendiente

**La compilación completa para Android y una prueba física de esta integración siguen pendientes de verificación.** Antes de publicar, es necesario ejecutar tests, compilar con el SDK/NDK del proyecto y probar inicio del juego, partidas guardadas, mochila, PC, Datos, joystick, sprites, vibración, multitouch y actualización sobre la APK oficial firmada.

El autor de la PR no solicita una fusión automática ni que Klisee cambie su clave de publicación.
