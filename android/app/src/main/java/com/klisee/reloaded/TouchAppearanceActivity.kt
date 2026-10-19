package com.klisee.reloaded

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import com.klisee.reloaded.ui.TouchAppearanceScreen
import com.klisee.reloaded.ui.ReloadedTheme

class TouchAppearanceActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { ReloadedTheme { TouchAppearanceScreen(Preferences(this)) { finish() } } }
    }
}
