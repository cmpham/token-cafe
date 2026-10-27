package cafe.token.android;

import android.app.Activity;
import android.app.AlertDialog;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;

public final class MainActivity extends Activity {
    private CafeView cafe;
    private TextView status;
    private ScheduledExecutorService polling;
    private boolean resumed;
    private int generation;
    private String token = "";
    private boolean demo = true;

    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        Object retained = getLastNonConfigurationInstance();
        if (retained instanceof String) { token = (String)retained; demo = token.isEmpty(); }
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(0xff151322);
        // Target 35 draws edge-to-edge: keep buttons and touch mapping inside system insets.
        root.setOnApplyWindowInsetsListener((v, insets) -> {
            v.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            return insets;
        });
        status = new TextView(this);
        status.setTextColor(0xffffd39a);
        status.setGravity(Gravity.CENTER);
        status.setPadding(12, 12, 12, 12);
        status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        root.addView(status);
        cafe = new CafeView(this);
        root.addView(cafe, new LinearLayout.LayoutParams(-1, 0, 1));
        LinearLayout controls = new LinearLayout(this);
        Button demoButton = new Button(this);
        demoButton.setText(R.string.demo);
        demoButton.setOnClickListener(v -> {
            stopPolling(); token = ""; demo = true;
            NativeCafe.reset(true);
            status.setText(R.string.demo_status);
        });
        Button usbButton = new Button(this);
        usbButton.setText(R.string.connect_usb);
        usbButton.setOnClickListener(v -> connectDialog());
        controls.addView(demoButton, new LinearLayout.LayoutParams(0, -2, 1));
        controls.addView(usbButton, new LinearLayout.LayoutParams(0, -2, 1));
        root.addView(controls);
        setContentView(root);
        NativeCafe.reset(demo);
    }

    @Override public Object onRetainNonConfigurationInstance() { return demo ? "" : token; }

    private void connectDialog() {
        EditText input = new EditText(this);
        input.setSingleLine(true);
        input.setHint(R.string.pairing_hint);
        input.setInputType(android.text.InputType.TYPE_CLASS_TEXT | android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD);
        AlertDialog dialog = new AlertDialog.Builder(this)
            .setTitle(R.string.connect_title)
            .setMessage(R.string.connect_message)
            .setView(input).setNegativeButton(R.string.cancel, null).setPositiveButton(R.string.connect, null).create();
        dialog.setOnShowListener(d -> dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v -> {
            String candidate = input.getText().toString().trim();
            if (!candidate.matches("[0-9a-f]{64}")) { input.setError(getString(R.string.pairing_error)); return; }
            stopPolling(); token = candidate; demo = false;
            NativeCafe.reset(false);
            if (resumed) startPolling();
            dialog.dismiss();
        }));
        dialog.show();
    }

    @Override protected void onResume() {
        super.onResume(); resumed = true; cafe.start();
        if (demo) status.setText(R.string.demo_status);
        else startPolling();
    }

    @Override protected void onPause() {
        resumed = false; cafe.stop(); stopPolling();
        super.onPause();
    }

    private void stopPolling() {
        generation++;
        if (polling != null) { polling.shutdownNow(); polling = null; }
    }

    private void startPolling() {
        stopPolling();
        final int attempt = generation;
        final String credential = token;
        status.setText(R.string.connecting);
        polling = Executors.newSingleThreadScheduledExecutor();
        polling.scheduleWithFixedDelay(() -> {
            HttpURLConnection connection = null;
            try {
                connection = (HttpURLConnection)new URL("http://127.0.0.1:38417/v1/snapshot").openConnection();
                connection.setConnectTimeout(2000);
                connection.setReadTimeout(2000);
                connection.setInstanceFollowRedirects(false);
                connection.setUseCaches(false);
                connection.setRequestProperty("Authorization", "Bearer " + credential);
                if (connection.getResponseCode() != 200) throw new IllegalStateException(getString(R.string.pairing_rejected));
                byte[] snapshot;
                try (InputStream stream = connection.getInputStream(); ByteArrayOutputStream out = new ByteArrayOutputStream()) {
                    byte[] buffer = new byte[8192];
                    int size;
                    while ((size = stream.read(buffer)) != -1) {
                        if (out.size() + size > 1024 * 1024) throw new IllegalStateException(getString(R.string.snapshot_too_large));
                        out.write(buffer, 0, size);
                    }
                    snapshot = out.toByteArray();
                }
                runOnUiThread(() -> {
                    if (attempt != generation || !resumed) return;
                    if (NativeCafe.snapshot(snapshot)) status.setText(R.string.live_status);
                    else disconnected(getString(R.string.incompatible));
                });
            } catch (Exception failure) {
                final String message = failure instanceof IllegalStateException
                    ? failure.getMessage() : getString(R.string.disconnected);
                runOnUiThread(() -> { if (attempt == generation && resumed) disconnected(message); });
            } finally { if (connection != null) connection.disconnect(); }
        }, 0, 1, TimeUnit.SECONDS);
    }

    private void disconnected(String message) {
        // Never leave old working/waiting patrons looking live after connection loss.
        NativeCafe.reset(false);
        status.setText(getString(R.string.retrying, message));
    }
}
