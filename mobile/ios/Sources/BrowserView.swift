import Combine
import SwiftUI
import WebKit

final class BrowserModel: NSObject, ObservableObject, WKNavigationDelegate {
    private static let home = "https://duckduckgo.com/"

    @Published var address = ""
    @Published var canGoBack = false
    @Published var canGoForward = false
    @Published var isLoading = false

    let webView: WKWebView

    override init() {
        let configuration = PrivacyConfiguration.makeWebViewConfiguration()
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init()

        PrivacyConfiguration.apply(to: webView)
        webView.navigationDelegate = self
        webView.allowsBackForwardNavigationGestures = true
        load(Self.home)
    }

    func load(_ rawInput: String) {
        let input = rawInput.trimmingCharacters(in: .whitespacesAndNewlines)
        let destination: String

        if input.isEmpty {
            destination = Self.home
        } else if let url = URL(string: input), let scheme = url.scheme, ["http", "https"].contains(scheme.lowercased()) {
            destination = input
        } else if input.contains(" ") || !input.contains(".") {
            let query = input.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? input
            destination = "https://duckduckgo.com/?q=\(query)"
        } else {
            destination = "https://\(input)"
        }

        guard let url = URL(string: destination) else { return }
        address = destination
        webView.load(URLRequest(url: url))
    }

    func goBack() {
        guard webView.canGoBack else { return }
        webView.goBack()
    }

    func goForward() {
        guard webView.canGoForward else { return }
        webView.goForward()
    }

    func reload() {
        webView.reload()
    }

    private func syncNavigationState() {
        address = webView.url?.absoluteString ?? address
        canGoBack = webView.canGoBack
        canGoForward = webView.canGoForward
        isLoading = webView.isLoading
    }

    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        syncNavigationState()
    }

    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
        syncNavigationState()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        syncNavigationState()
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        syncNavigationState()
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        syncNavigationState()
    }
}

struct BrowserWebView: UIViewRepresentable {
    @ObservedObject var model: BrowserModel

    func makeUIView(context: Context) -> WKWebView {
        model.webView
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {}
}

struct BrowserView: View {
    @StateObject private var model = BrowserModel()

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Button(action: model.goBack) {
                    Image(systemName: "chevron.left")
                }
                .disabled(!model.canGoBack)

                Button(action: model.goForward) {
                    Image(systemName: "chevron.right")
                }
                .disabled(!model.canGoForward)

                Button(action: model.reload) {
                    Image(systemName: model.isLoading ? "xmark" : "arrow.clockwise")
                }

                TextField("Search or enter address", text: $model.address)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled(true)
                    .keyboardType(.webSearch)
                    .submitLabel(.go)
                    .onSubmit {
                        model.load(model.address)
                    }
                    .padding(.horizontal, 12)
                    .frame(height: 40)
                    .background(Color.white.opacity(0.08))
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            .buttonStyle(.plain)
            .foregroundStyle(.white)
            .padding(8)
            .background(Color.black)

            BrowserWebView(model: model)
                .ignoresSafeArea(.container, edges: .bottom)
        }
        .background(Color.black)
    }
}
