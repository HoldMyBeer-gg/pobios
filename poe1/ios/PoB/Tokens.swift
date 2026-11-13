import SwiftUI

/// The Ascent design system, as the canvas uses it.
///
/// Values are the ones in `_ds/.../tokens/*.css`, kept by their token names
/// so a change in the design maps to one line here. Cold stone ground with a
/// blue undertone; electric cyan means "the current action"; ice is
/// emphasis; coral is warnings only.
enum Tok {
    /// The active theme. Every colour token reads from this, so switching it
    /// (Settings) reskins the whole SwiftUI shell. The tree canvas is a
    /// deliberate in-game replica and is not themed.
    static var current: Theme = Theme.load()

    // Ground ramp
    static var ink000: Color { current.ink000 }   // behind sheets, scrim base
    static var ink050: Color { current.ink050 }   // app ground
    static var ink100: Color { current.ink100 }   // wells, inputs
    static var ink200: Color { current.ink200 }
    static var ink300: Color { current.ink300 }   // card surface
    static var ink400: Color { current.ink400 }   // hairline inside a card
    static var ink500: Color { current.ink500 }   // border, default
    static var ink600: Color { current.ink600 }   // border, strong / hover
    /// The chrome bars and sidebar sit one step off the ground.
    static var bar: Color { current.bar }

    // Electric cyan (accent)
    static var cyan300: Color { current.accentText }   // accent text, focus ring
    static var cyan400: Color { current.accentHover }  // hover
    static var cyan500: Color { current.accent }       // primary
    static var cyan600: Color { current.accentPress }  // press
    static var cyan700: Color { current.accentRest }   // at rest
    static var cyan800: Color { current.accentTint }   // tint fill

    // Ice: emphasis
    static var ice300: Color { current.ice300 }
    static var ice500: Color { current.ice500 }
    static var ice600: Color { current.ice600 }
    static var ice700: Color { current.ice700 }   // section labels at rest

    // Text ramp
    static var textPrimary: Color   { current.textPrimary }
    static var textSecondary: Color { current.textSecondary }
    static var textTertiary: Color  { current.textTertiary }
    static var textDisabled: Color  { current.textDisabled }

    // Status
    static var violet400: Color { current.violet }    // unique items, "new"
    static var coral300: Color  { current.coral300 }  // warning text
    static var coral500: Color  { current.coral500 }  // danger
    static var ok: Color        { current.ok }
    static var dangerTint: Color { current.dangerTint } // warning/negative panel fill

    // Atmosphere: the app-ground radial gradient, top-lit.
    static var groundTop: Color { current.groundTop }
    static var groundMid: Color { current.groundMid }
    static var groundBot: Color { current.groundBot }

    // Sockets: PoE's own colours, never themed (they mirror the game).
    static let socketRed   = Color(hex: 0xFF5D78)
    static let socketGreen = Color(hex: 0x43D6A3)
    static let socketBlue  = Color(hex: 0x4D9BFF)
    static let socketWhite = Color(hex: 0xDFE7F5)

    enum Radius {
        static let sm: CGFloat = 3   // chips, pips
        static let md: CGFloat = 5   // plates, inputs, buttons
        static let lg: CGFloat = 8   // sheets
    }

    enum Space {
        static let s1: CGFloat = 4
        static let s2: CGFloat = 8
        static let s3: CGFloat = 12
        static let s4: CGFloat = 16
        static let s5: CGFloat = 20
        static let s6: CGFloat = 24
    }

    /// Fixed heights from the canvas.
    enum Size {
        static let topBar: CGFloat = 52
        static let sidebar: CGFloat = 276
        static let control: CGFloat = 36
        static let touchMin: CGFloat = 44
    }

    // Type. The canvas pairs Space Grotesk (display), IBM Plex Sans (UI) and
    // JetBrains Mono (numerals). Until those ship in the bundle the system
    // faces stand in at the same sizes; every numeral is monospaced so
    // columns still align.
    /// Dark drop shadow under light-on-dark text (skill and gem names), a
    /// trial from 2026-09-24 to see whether it separates the type from the
    /// card. The style rule keeps shadows off surfaces; this is on glyphs.
    static let textLift = TextLift()

    static func display(_ size: CGFloat, _ weight: Font.Weight = .bold) -> Font {
        .system(size: size, weight: weight, design: .default)
    }
    static func ui(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight)
    }
    static func mono(_ size: CGFloat, _ weight: Font.Weight = .medium) -> Font {
        .system(size: size, weight: weight, design: .monospaced)
    }
    /// All-caps tracked label, the section-header voice.
    static let label = ui(11, .bold)
    static let labelTracking: CGFloat = 2.2
}

extension Color {
    init(hex: UInt32, alpha: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: alpha
        )
    }

    /// PoB's own colours arrive as `#rrggbb` from the bridge.
    /// A PoB colour code as a SwiftUI colour. PoB's plain white is the
    /// theme's primary text, not literal white, so a warm-text theme
    /// warms the numbers too and not only the labels.
    init(rrggbb: String) {
        var s = rrggbb
        if s.hasPrefix("#") { s.removeFirst() }
        let v = UInt32(s, radix: 16) ?? 0xFFFFFF
        if v == 0xFFFFFF || v == 0xE8ECF4 { self = Tok.textPrimary } else { self.init(hex: v) }
    }
}

// MARK: - Shared chrome pieces

/// A plate-style control: the bordered pill every toolbar in the canvas uses.
struct PillStyle: ButtonStyle {
    var primary = false
    var danger = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Tok.ui(15, primary ? .bold : .medium))
            .foregroundStyle(primary ? Tok.ink050 : danger ? Tok.coral300 : Tok.textPrimary)
            .padding(.horizontal, Tok.Space.s4)
            .frame(height: Tok.Size.touchMin)
            .background(primary ? Tok.cyan500 : Tok.ink300.opacity(configuration.isPressed ? 0.6 : 1))
            .overlay(
                RoundedRectangle(cornerRadius: Tok.Radius.md)
                    .stroke(danger ? Tok.coral500 : primary ? Tok.cyan400 : Tok.ink500, lineWidth: 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
            .opacity(configuration.isPressed ? 0.85 : 1)
    }
}

/// The dropdown pill: label plus a chevron. Wraps a `Menu` when there is
/// something to pick, or reads as a static pill when the bridge has no setter
/// for it yet.
struct DropPill<Content: View>: View {
    let title: String
    var enabled = true
    /// Cap the label; long set and loadout names truncate rather than
    /// pushing the rest of a toolbar off the edge.
    var maxWidth: CGFloat? = nil
    @ViewBuilder var content: Content

    var body: some View {
        Menu {
            content
        } label: {
            HStack(spacing: Tok.Space.s2) {
                Text(title)
                    .lineLimit(1).truncationMode(.tail)
                    .frame(maxWidth: maxWidth, alignment: .leading)
                Image(systemName: "chevron.down")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(enabled ? Tok.textPrimary : Tok.textTertiary)
            }
        }
        .buttonStyle(PillStyle())
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.7)
    }
}

/// "OFFENCE ────── ▾" : the section header voice used in the sidebar and cards.
struct SectionLabel: View {
    let text: String
    var body: some View {
        Text(text.uppercased())
            .font(Tok.label)
            .tracking(Tok.labelTracking)
            .foregroundStyle(Tok.ice700)
    }
}

/// A text field drawn as a well, matching the canvas's inputs.
struct WellField: View {
    let placeholder: String
    @Binding var text: String
    var body: some View {
        TextField(placeholder, text: $text)
            .textFieldStyle(.plain)
            .font(Tok.ui(15))
            .foregroundStyle(Tok.textPrimary)
            .padding(.horizontal, Tok.Space.s3)
            .frame(height: Tok.Size.touchMin)
            .background(Tok.ink100)
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
            .autocorrectionDisabled()
            .textInputAutocapitalization(.never)
    }
}

/// A square icon button in the pill family, for undo/redo and the like.
struct IconPillStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(enabled ? Tok.textPrimary : Tok.textDisabled)
            .frame(width: Tok.Size.touchMin, height: Tok.Size.touchMin)
            .background(Tok.ink300.opacity(configuration.isPressed ? 0.6 : 1))
            .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
    }
}

struct TextLift: ViewModifier {
    func body(content: Content) -> some View {
        content.shadow(color: .black.opacity(0.7), radius: 1, x: 0, y: 2)
    }
}

extension View {
    /// Apply `Tok.textLift` to skill and gem names.
    func textLift() -> some View { modifier(Tok.textLift) }
}
