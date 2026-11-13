import UIKit
import MetalKit

/// Hosts the Rust app on an MTKView.
///
/// UIKit drives the frames: `MTKViewDelegate.draw(in:)` calls into Rust once
/// per display refresh. Rust does not run a loop of its own, which is exactly
/// what lets the app live inside a scene the system controls and resizes.
final class PobViewController: UIViewController, MTKViewDelegate,
                               UIGestureRecognizerDelegate {
    private var handle: UnsafeMutableRawPointer?
    private var metalView: MTKView!
    /// Set while the system is interactively resizing the scene. Apple's
    /// guidance for Metal apps is to keep drawing at the current size during
    /// a resize and reconfigure only when it ends; reallocating the swapchain
    /// every frame of a drag is explicitly the wrong thing.
    private var resizing = false

    /// The live Rust handle, for `EguiBridge`.
    var handleForBridge: UnsafeMutableRawPointer? { handle }

    /// Draw a frame soon. Input from outside the canvas still has to reach
    /// the screen.
    func wake() {
        metalView?.isPaused = false
    }

    /// See `EguiView.primary`.
    let primary: Bool

    init(primary: Bool) {
        self.primary = primary
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not from a storyboard") }

    override func viewDidLoad() {
        super.viewDidLoad()
        if primary { EguiBridge.controller = self }

        metalView = MTKView(frame: view.bounds)
        metalView.device = MTLCreateSystemDefaultDevice()
        metalView.delegate = self
        metalView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        // The layer is handed to Rust, which draws into it directly.
        metalView.framebufferOnly = true
        metalView.isMultipleTouchEnabled = true
        view.addSubview(metalView)

        // Two-finger scroll and pinch. Single-finger drags stay as touches so
        // they can reach egui as press-and-drag, which is what the splitters,
        // sliders and the tree canvas all expect.
        let pan = UIPanGestureRecognizer(target: self, action: #selector(onPan))
        pan.minimumNumberOfTouches = 2
        pan.maximumNumberOfTouches = 2
        metalView.addGestureRecognizer(pan)

        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(onPinch))
        // Both recognisers must run together: a pinch is also a two-finger
        // pan, and iOS would otherwise let one cancel the other.
        pinch.delegate = self
        pan.delegate = self
        metalView.addGestureRecognizer(pinch)

        observeKeyboard()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        if handle == nil {
            start()
        } else if !resizing {
            resize()
            // A resize with no touch following it (the sidebar collapsing,
            // a split-view drag) needs a frame or the canvas keeps the old
            // one stretched.
            metalView.isPaused = false
        }
    }

    /// Rotation and window resizing both arrive here.
    ///
    /// Apple's guidance for Metal apps is to keep drawing at the current size
    /// while a resize is in progress and reconfigure when it finishes, so the
    /// swapchain is not reallocated on every frame of an animation.
    override func viewWillTransition(
        to size: CGSize,
        with coordinator: UIViewControllerTransitionCoordinator
    ) {
        super.viewWillTransition(to: size, with: coordinator)
        resizing = true
        metalView?.isPaused = false
        coordinator.animate(alongsideTransition: nil) { [weak self] _ in
            self?.resizing = false
            self?.resize()
            self?.metalView?.isPaused = false
        }
    }

    /// The keyboard changes how much room this view has, and the tree has to
    /// be told or it keeps drawing at the old size — squashed while the
    /// keyboard is up, and stretched after it goes away.
    ///
    /// Both notifications matter: `willChangeFrame` covers the keyboard
    /// resizing itself, which happens when a hardware keyboard connects or
    /// the floating one is dragged.
    private func observeKeyboard() {
        for name in [
            UIResponder.keyboardDidShowNotification,
            UIResponder.keyboardDidHideNotification,
            UIResponder.keyboardDidChangeFrameNotification,
            // Rotation changes the drawable size the same way the keyboard
            // does, and the canvas has to be told or it keeps drawing at the
            // old aspect — letterboxed one way, cropped the other.
            UIDevice.orientationDidChangeNotification,
        ] {
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(layoutChanged),
                name: name,
                object: nil
            )
        }
    }

    @objc private func layoutChanged() {
        // Next runloop pass: the view's own layout has not settled at the
        // moment the notification fires, so measuring now reports the size
        // that is about to be replaced.
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.view.setNeedsLayout()
            self.view.layoutIfNeeded()
            self.resize()
            self.metalView.isPaused = false
        }
    }

    private func start() {
        guard let layer = metalView.layer as? CAMetalLayer else { return }

        // Settings live in the sandbox: the bundle is read-only on device,
        // and writing there breaks the code signature.
        let support = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(
            at: support, withIntermediateDirectories: true)

        let scale = Float(metalView.contentScaleFactor)
        let size = metalView.drawableSize
        guard size.width > 0, size.height > 0 else { return }

        // PoB's Lua and data ship in the bundle as a folder reference, so
        // the nesting its `require` paths depend on survives. The Rust side
        // has no baked-in path — the build machine's directories do not
        // exist here.
        guard let data = Bundle.main.resourceURL?.appendingPathComponent("src") else {
            return
        }

        handle = support.path.withCString { dir in
            data.path.withCString { dataDir in
                pobng_start(
                    Unmanaged.passUnretained(layer).toOpaque(),
                    dir,
                    dataDir,
                    UInt32(size.width),
                    UInt32(size.height),
                    scale
                )
            }
        }

        if handle == nil {
            // Nothing can be drawn without a GPU surface, and a blank screen
            // with no explanation is the worst possible failure.
            let label = UILabel(frame: view.bounds)
            label.text = "Could not start the renderer."
            label.textAlignment = .center
            label.textColor = UIColor(Tok.textPrimary)
            view.backgroundColor = UIColor(Tok.ink000)
            view.addSubview(label)
        }
    }

    private func resize() {
        guard let handle else { return }
        let size = metalView.drawableSize
        guard size.width > 0, size.height > 0 else { return }
        pobng_resize(
            handle,
            UInt32(size.width),
            UInt32(size.height),
            Float(metalView.contentScaleFactor)
        )
    }

    // MARK: Input

    // Touches are forwarded as pointer events: this UI was built for a mouse,
    // and its controls want hover, press and release rather than egui's own
    // touch events.

    /// How many fingers are down. A second one turns the gesture into a
    /// pinch or a scroll, and the pointer events have to stop.
    private var activeTouches = 0

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        activeTouches += touches.count
        if activeTouches > 1 {
            // The single-finger press already sent has to be taken back, or
            // egui holds a button down for the whole pinch.
            if let handle { pobng_touch(handle, 3, 0, 0) }
            return
        }
        send(touches, phase: 0)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        send(touches, phase: 1)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        activeTouches = max(0, activeTouches - touches.count)
        send(touches, phase: 2)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        activeTouches = max(0, activeTouches - touches.count)
        send(touches, phase: 3)
    }

    private func send(_ touches: Set<UITouch>, phase: UInt32) {
        guard let handle, let touch = touches.first else { return }
        // Two fingers down means a pinch or a scroll, not a click. Forwarding
        // it as a pointer press makes egui think a drag started, which fights
        // the gesture and was why pinch appeared to do nothing.
        if activeTouches > 1 { return }
        let p = touch.location(in: metalView)
        pobng_touch(handle, phase, Float(p.x), Float(p.y))
        // Input only matters if a frame follows it.
        metalView.isPaused = false
    }

    @objc private func onPan(_ g: UIPanGestureRecognizer) {
        guard let handle else { return }
        let d = g.translation(in: metalView)
        g.setTranslation(.zero, in: metalView)
        pobng_scroll(handle, Float(d.x), Float(d.y))
        metalView.isPaused = false
    }

    @objc private func onPinch(_ g: UIPinchGestureRecognizer) {
        guard let handle else { return }
        // Reported relative to the last event rather than the gesture's start,
        // so egui accumulates it the same way a trackpad pinch would.
        pobng_zoom(handle, Float(g.scale))
        g.scale = 1.0
        metalView.isPaused = false
    }

    /// Let pinch and pan run at the same time: a two-finger gesture is both.
    func gestureRecognizer(
        _ g: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
    ) -> Bool {
        true
    }

    // MARK: MTKViewDelegate

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
        // Handled in viewDidLayoutSubviews, where the resize state is known.
    }

    func draw(in view: MTKView) {
        guard let handle else { return }
        let delay = pobng_frame(handle)
        // The canvas may have changed the build (a node was tapped). Let the
        // model check, after the frame so the engine is settled.
        EguiBridge.frameDidDraw?()
        // The app says when it next needs drawing. A static screen should not
        // cost 120Hz of battery.
        view.isPaused = delay == UInt32.max
        if !view.isPaused {
            view.preferredFramesPerSecond = delay > 50 ? 20 : 60
        }
    }

    deinit {
        if let handle {
            pobng_stop(handle)
        }
    }
}

/// Reaching the egui canvas from SwiftUI.
///
/// The canvas is created by a `UIViewControllerRepresentable`, so SwiftUI has
/// no direct reference to it. This holds the live handle so views like the
/// tree search can talk to it without threading a binding through every
/// layer.
enum EguiBridge {
    static weak var controller: PobViewController?
    /// Called after every drawn frame. `BuildModel` uses it to notice
    /// changes the canvas made and re-read the build.
    static var frameDidDraw: (() -> Void)?

    /// The node whose tooltip is pinned on the canvas, if any.
    static func pinnedNode() -> UInt64? {
        guard let handle = controller?.handleForBridge else { return nil }
        let id = pobng_pinned_node(handle)
        return id == 0 ? nil : id
    }

    /// Drop the pinned tooltip and redraw.
    static func unpin() {
        guard let handle = controller?.handleForBridge else { return }
        pobng_unpin(handle)
        controller?.wake()
    }

    /// Centre the canvas on a node and pin its tooltip.
    static func focusNode(_ id: UInt64) {
        guard let handle = controller?.handleForBridge else { return }
        pobng_focus_node(handle, id)
        controller?.wake()
    }

    /// Show a node ranking on the canvas, or clear it with an empty list.
    static func setPower(_ ids: [UInt64], _ values: [Float]) {
        guard let handle = controller?.handleForBridge else { return }
        ids.withUnsafeBufferPointer { i in
            values.withUnsafeBufferPointer { v in
                pobng_set_power(handle, i.baseAddress, v.baseAddress, min(i.count, v.count))
            }
        }
        controller?.wake()
    }

    static func setSearch(_ ids: [UInt64], searching: Bool) {
        guard let handle = controller?.handleForBridge else { return }
        ids.withUnsafeBufferPointer { buf in
            pobng_set_search(handle, buf.baseAddress, ids.count, searching)
        }
        controller?.wake()
    }
}
