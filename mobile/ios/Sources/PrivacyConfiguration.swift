import WebKit

enum PrivacyConfiguration {
    static func makeWebViewConfiguration() -> WKWebViewConfiguration {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = true
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        return configuration
    }

    static func apply(to webView: WKWebView) {
        if #available(iOS 16.4, *) {
            webView.isInspectable = false
        }
    }
}
