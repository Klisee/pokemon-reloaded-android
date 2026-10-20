package com.klisee.reloaded

import org.junit.Assert.*
import org.junit.Test

class TouchUpdateTest {
    @Test fun oldLayoutsAndNewButtonRoundTripWithoutResettingOtherProfiles() {
        val old = TouchLayouts.parse("""touch-layout-v1
world z 0.78 0.84 1.4 1
battle a 0.7 0.4 1.2 0
world joystick 0.15 0.65 0.8 1
""")
        val world = TouchLayouts.controlsFor(TouchProfile.WORLD, old)
        assertTrue(world.any { it.id == "alpha8" })
        assertTrue(world.any { it.id == "sel_world" })
        assertFalse(world.any { it.id in (1..7).map { n -> "alpha$n" } })
        assertEquals(TouchPlacement(.78f, .84f, 1.4f, true), old["world/z"])
        assertEquals(TouchPlacement(.15f, .65f, .8f, true), old["world/joystick"])
        val eight = world.single { it.id == "alpha8" }
        assertEquals("8", eight.label)
        assertEquals("alpha8.png", eight.asset)
        assertEquals(TouchPlacement(.228f, .05f), TouchLayouts.original(eight))
        val edited = old + ("world/alpha8" to TouchPlacement(.4f, .2f, 1.5f, false))
        assertEquals(edited, TouchLayouts.parse(TouchLayouts.encode(edited)))
        assertEquals(mapOf("battle/a" to old.getValue("battle/a")), TouchLayouts.resetProfile(edited, TouchProfile.WORLD))
    }

    @Test fun backpackGetsOnlyJoystickZAndXByDefault() {
        assertEquals(setOf("joystick", "z", "x"), TouchLayouts.controlsFor(TouchProfile.MENU_NAV).map { it.id }.toSet())
        assertTrue(TouchLayouts.controlsFor(TouchProfile.MENU).isEmpty())
        val battle = TouchLayouts.controlsFor(TouchProfile.BATTLE).map { it.id }
        assertTrue("pkmn" in battle)
        assertFalse("alpha8" in battle || "sel_world" in battle)
    }

    @Test fun button8KeepsPkmSizeAndTopRowPositionAcrossLandscapeResolutions() {
        val eight = TouchLayouts.controls.single { it.id == "alpha8" }
        val pkm = TouchLayouts.controls.single { it.id == "space" }
        for ((w, h) in listOf(1536f to 720f, 2400f to 1080f, 1600f to 1200f)) {
            val a = TouchLayouts.geometry(eight, TouchLayouts.original(eight), w, h, false)
            val b = TouchLayouts.geometry(pkm, TouchLayouts.original(pkm), w, h, false)
            assertEquals(b.radius, a.radius, .01f)
            assertEquals(b.y, a.y, .01f)
            assertTrue(a.x + a.radius < b.x - b.radius)
            assertTrue(a.y - a.radius >= 0f)
        }
    }
}
