package app.veil.browser;

import android.app.Activity;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.view.Gravity;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;

import org.mozilla.geckoview.GeckoRuntime;
import org.mozilla.geckoview.GeckoSession;
import org.mozilla.geckoview.GeckoView;

public final class MainActivity extends Activity {
    private static final String HOME = "https://duckduckgo.com/";

    private GeckoRuntime runtime;
    private GeckoSession session;
    private EditText address;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        runtime = GeckoRuntime.create(this);
        session = new GeckoSession();
        session.open(runtime);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.BLACK);

        LinearLayout toolbar = new LinearLayout(this);
        toolbar.setOrientation(LinearLayout.HORIZONTAL);
        toolbar.setGravity(Gravity.CENTER_VERTICAL);
        toolbar.setPadding(dp(8), dp(8), dp(8), dp(8));
        toolbar.setBackgroundColor(Color.BLACK);

        Button back = navButton("‹");
        back.setOnClickListener(view -> session.goBack());
        toolbar.addView(back);

        Button forward = navButton("›");
        forward.setOnClickListener(view -> session.goForward());
        toolbar.addView(forward);

        Button reload = navButton("↻");
        reload.setOnClickListener(view -> session.reload());
        toolbar.addView(reload);

        address = new EditText(this);
        address.setSingleLine(true);
        address.setTextColor(Color.WHITE);
        address.setHintTextColor(Color.GRAY);
        address.setBackgroundColor(Color.rgb(24, 24, 24));
        address.setHint("Search or enter address");
        address.setPadding(dp(12), 0, dp(12), 0);
        address.setImeOptions(EditorInfo.IME_ACTION_GO);
        address.setOnEditorActionListener((view, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_GO) {
                navigate(address.getText().toString());
                return true;
            }
            return false;
        });
        toolbar.addView(address, new LinearLayout.LayoutParams(0, dp(44), 1f));

        GeckoView geckoView = new GeckoView(this);
        geckoView.setSession(session);

        root.addView(toolbar, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        ));
        root.addView(geckoView, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                0,
                1f
        ));

        setContentView(root);
        session.loadUri(HOME);
    }

    private Button navButton(String label) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextColor(Color.WHITE);
        button.setBackgroundColor(Color.BLACK);
        button.setMinWidth(dp(48));
        button.setMinimumWidth(dp(48));
        return button;
    }

    private void navigate(String rawInput) {
        String input = rawInput == null ? "" : rawInput.trim();
        if (input.isEmpty()) {
            session.loadUri(HOME);
            return;
        }

        final String destination;
        if (input.matches("^[A-Za-z][A-Za-z0-9+.-]*://.*")) {
            destination = input;
        } else if (input.contains(" ") || !input.contains(".")) {
            destination = "https://duckduckgo.com/?q=" + Uri.encode(input);
        } else {
            destination = "https://" + input;
        }

        address.setText(destination);
        session.loadUri(destination);
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
