import UIKit
import SwiftUI

/// Owns the window and the SwiftUI root.
class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }

        // The design is laid out for 1194x834 (iPad Pro 11" landscape). A
        // preference, not a guarantee: the system may still hand over less.
        windowScene.sizeRestrictions?.minimumSize = CGSize(width: 960, height: 600)

        let window = UIWindow(windowScene: windowScene)
        window.rootViewController = UIHostingController(rootView: LaunchView())
        window.overrideUserInterfaceStyle = .dark
        window.makeKeyAndVisible()
        self.window = window

        // A pob:// link that launched the app.
        if let url = connectionOptions.urlContexts.first?.url {
            LinkRouter.shared.deliver(url)
        }
    }

    /// A pob:// link opened while the app is running.
    func scene(_ scene: UIScene, openURLContexts URLContexts: Set<UIOpenURLContext>) {
        if let url = URLContexts.first?.url {
            LinkRouter.shared.deliver(url)
        }
    }
}
