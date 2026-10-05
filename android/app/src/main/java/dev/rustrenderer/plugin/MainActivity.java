package dev.rustrenderer.plugin;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;
import android.widget.ScrollView;
import android.widget.LinearLayout;
import android.widget.Button;
import android.widget.EditText;
import android.widget.CheckBox;
import android.widget.LinearLayout.LayoutParams;
import android.view.Gravity;
import android.content.Intent;
import android.net.Uri;

public class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle b) {
        super.onCreate(b);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(48, 96, 48, 48);
        root.setGravity(Gravity.TOP);

        TextView title = new TextView(this);
        title.setTextSize(22);
        title.setText("RustGL Plugin");
        root.addView(title);

        TextView info = new TextView(this);
        info.setText("\nRenderer options (env vars):\n"
            + "RENDERER_BACKEND=gles|vulkan|hybrid|auto\n"
            + "RENDERER_BACKEND_SELECT=gles|vulkan|hybrid|auto (legacy)\n"
            + "RENDERER_DISPLAY=\n"
            + "RENDERER_ANGLE_BACKEND=\n"
            + "RENDERER_ANGLE_RENDERER=\n"
            + "RENDERER_DEBUG=1\n"
            + "RENDERER_SPOOF_GL=1\n"
            + "RENDERER_TRACE_GL=1\n"
            + "RENDERER_DUMP_SHADER_DIR=/path/to/dir\n"
            + "\nSet these in ZalithLauncher renderer settings.");
        root.addView(info);

        Button docs = new Button(this);
        docs.setText("Open docs");
        docs.setOnClickListener(v -> {
            Intent i = new Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/WhalterMC/rust-renderer"));
            startActivity(i);
        });
        root.addView(docs);

        setContentView(root);
    }
}
