package dev.rustrenderer.plugin;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;

public class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle b) {
        super.onCreate(b);
        TextView t = new TextView(this);
        t.setPadding(48, 96, 48, 48);
        t.setText("RustGL plugin\n\nThis app is a renderer plugin for ZalithLauncher 2. "
            + "Open the launcher and pick \"RustGL\" in renderer settings.");
        setContentView(t);
    }
}
