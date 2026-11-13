import UIKit

/// The app entry point.
///
/// Swift owns `UIApplicationMain` rather than Rust, which is what lets the
/// app use UIKit's scene lifecycle. winit cannot be embedded, and from the
/// iOS 27 SDK an app without the scene lifecycle does not launch at all.
@main
class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        UISceneConfiguration(name: "Default Configuration", sessionRole: session.role)
    }
}
