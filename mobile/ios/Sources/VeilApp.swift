import SwiftUI

@main
struct VeilApp: App {
    var body: some Scene {
        WindowGroup {
            BrowserView()
                .preferredColorScheme(.dark)
        }
    }
}
