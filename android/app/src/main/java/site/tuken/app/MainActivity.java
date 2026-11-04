package site.tuken.app;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * 兔啃 on the phone: www.tuken.site/remote (手机遥控, check-in, balance) in
 * an app window. The page keeps the pairing in its own storage; this app adds
 * the home screen icon, the back button, and the pairing link opening here.
 */
public class MainActivity extends Activity {
    private static final String HOME = "https://www.tuken.site/remote";
    private static final int BACKGROUND = 0xFFF2F7F6;
    private static final int INK = 0xFF003F43;

    private WebView web;
    private View offline;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        getWindow().setStatusBarColor(BACKGROUND);
        getWindow().setNavigationBarColor(Color.WHITE);

        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(BACKGROUND);
        root.setFitsSystemWindows(true);
        web = new WebView(this);
        web.setBackgroundColor(BACKGROUND);
        root.addView(web, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        offline = offlineView();
        offline.setVisibility(View.GONE);
        root.addView(offline, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        setContentView(root);

        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setAllowFileAccess(false);
        s.setAllowContentAccess(false);
        s.setMediaPlaybackRequiresUserGesture(true);
        // Large system fonts would break the page's layout; it has its own sizes.
        s.setTextZoom(100);
        // The page knows it runs in the app (no "download the app" link).
        s.setUserAgentString(s.getUserAgentString() + " TukenApp/" + version());

        web.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                Uri uri = request.getUrl();
                if (inApp(uri)) {
                    return false;
                }
                openOutside(uri);
                return true;
            }

            @Override
            public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame()) {
                    offline.setVisibility(View.VISIBLE);
                }
            }
        });

        if (saved != null) {
            web.restoreState(saved);
        } else {
            web.loadUrl(start(getIntent()));
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        Uri uri = intent.getData();
        // A pairing link: the page takes the token from it.
        if (uri != null && inApp(uri)) {
            web.loadUrl(uri.toString());
        }
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        web.saveState(out);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (offline.getVisibility() == View.VISIBLE) {
            offline.setVisibility(View.GONE);
        }
        if (web.canGoBack()) {
            web.goBack();
        } else {
            super.onBackPressed();
        }
    }

    @Override
    protected void onDestroy() {
        if (web != null) {
            web.destroy();
        }
        super.onDestroy();
    }

    private String start(Intent intent) {
        Uri uri = intent == null ? null : intent.getData();
        return uri != null && inApp(uri) ? uri.toString() : HOME;
    }

    /** The phone page stays here; everything else opens in the browser. */
    private static boolean inApp(Uri uri) {
        return "https".equals(uri.getScheme())
                && "www.tuken.site".equals(uri.getHost())
                && uri.getPath() != null
                && (uri.getPath().equals("/remote") || uri.getPath().startsWith("/remote/"));
    }

    private void openOutside(Uri uri) {
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        } catch (ActivityNotFoundException ignored) {
            // Nothing can open it; stay on the page.
        }
    }

    private String version() {
        try {
            return getPackageManager().getPackageInfo(getPackageName(), 0).versionName;
        } catch (Exception e) {
            return "0";
        }
    }

    private View offlineView() {
        LinearLayout box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setGravity(Gravity.CENTER);
        box.setBackgroundColor(BACKGROUND);
        int pad = dp(32);
        box.setPadding(pad, pad, pad, pad);

        TextView title = new TextView(this);
        title.setText("连不上兔啃");
        title.setTextColor(INK);
        title.setTextSize(20);
        title.setGravity(Gravity.CENTER);
        box.addView(title);

        TextView hint = new TextView(this);
        hint.setText("请检查网络后重试");
        hint.setTextColor(0xA6003F43);
        hint.setTextSize(15);
        hint.setGravity(Gravity.CENTER);
        hint.setPadding(0, dp(8), 0, dp(24));
        box.addView(hint);

        Button retry = new Button(this);
        retry.setText("重试");
        retry.setTextColor(Color.WHITE);
        retry.setBackgroundColor(0xFFED6C00);
        retry.setAllCaps(false);
        retry.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                offline.setVisibility(View.GONE);
                String url = web.getUrl();
                if (url == null || !inApp(Uri.parse(url))) {
                    web.loadUrl(HOME);
                } else {
                    web.reload();
                }
            }
        });
        box.addView(retry, new LinearLayout.LayoutParams(dp(160), dp(48)));
        return box;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
