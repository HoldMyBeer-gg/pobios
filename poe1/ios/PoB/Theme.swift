import SwiftUI

/// A colour palette for the SwiftUI shell. Every `Tok` colour reads from the
/// active `Theme`; spacing, radii, sizes and the socket/game colours are not
/// themed. The tree canvas is a deliberate in-game replica and keeps its own
/// palette regardless of the theme chosen here.
struct Theme: Identifiable, Equatable {
    let id: String
    let name: String

    // Ground ramp (darkest to lightest surface)
    let ink000, ink050, ink100, ink200, ink300, ink400, ink500, ink600, bar: Color
    // Accent (our cyan; Solarized's own accent in the Solarized themes)
    let accentText, accentHover, accent, accentPress, accentRest, accentTint: Color
    // Ice: high-emphasis text
    let ice300, ice500, ice600, ice700: Color
    // Text ramp
    let textPrimary, textSecondary, textTertiary, textDisabled: Color
    // Status
    let violet, coral300, coral500, ok, dangerTint: Color
    // App-ground gradient
    let groundTop, groundMid, groundBot: Color

    // Solarized Light is defined below but left out of the picker: its
    // low-contrast palette is hard to read (pale blue nav, yellow text on
    // cream). Add `.solarizedLight` back here to offer it.
    static let all: [Theme] = [.default_, .warm, .solarizedDark]

    // MARK: Persistence

    private static let key = "appTheme"

    static func load() -> Theme {
        let id = UserDefaults.standard.string(forKey: key)
        return all.first { $0.id == id } ?? .default_
    }

    static func set(_ theme: Theme) {
        UserDefaults.standard.set(theme.id, forKey: key)
        Tok.current = theme
    }
}

extension Theme {
    /// The Ascent default: cold blue-ink ground, electric cyan accent.
    /// These are the exact values the app shipped with.
    static let default_ = Theme(
        id: "default", name: "Ascent (default)",
        ink000: Color(hex: 0x05090F), ink050: Color(hex: 0x08101C), ink100: Color(hex: 0x0B1420),
        ink200: Color(hex: 0x0D1523), ink300: Color(hex: 0x101828), ink400: Color(hex: 0x162032),
        ink500: Color(hex: 0x1A2438), ink600: Color(hex: 0x26334D), bar: Color(hex: 0x0A1220),
        accentText: Color(hex: 0x9BE3FF), accentHover: Color(hex: 0x7DDAFF), accent: Color(hex: 0x3EC8FF),
        accentPress: Color(hex: 0x1C9FD6), accentRest: Color(hex: 0x14607F), accentTint: Color(hex: 0x0B2634),
        ice300: Color(hex: 0xF4F7FC), ice500: Color(hex: 0xCDD8EC), ice600: Color(hex: 0x8A99B8), ice700: Color(hex: 0x5B6B8A),
        textPrimary: Color(hex: 0xE8ECF4), textSecondary: Color(hex: 0x8A99B8),
        textTertiary: Color(hex: 0x5B6B8A), textDisabled: Color(hex: 0x3E4C66),
        violet: Color(hex: 0xC88FFF), coral300: Color(hex: 0xFF7D94), coral500: Color(hex: 0xFF4D6D),
        ok: Color(hex: 0x43D6A3), dangerTint: Color(hex: 0x3D1420),
        groundTop: Color(hex: 0x102039), groundMid: Color(hex: 0x0A1424), groundBot: Color(hex: 0x060B14)
    )

    /// Ascent with warm text: the same ground and accent, the white text
    /// pulled down to a parchment tone and a shade dimmer. Pure white on
    /// dark blue halos for some eyes (astigmatism, monovision); a warm
    /// off-white at a little less contrast reads cleaner without going grey.
    static let warm = Theme(
        id: "warm", name: "Ascent (warm text)",
        ink000: default_.ink000, ink050: default_.ink050, ink100: default_.ink100,
        ink200: default_.ink200, ink300: default_.ink300, ink400: default_.ink400,
        ink500: default_.ink500, ink600: default_.ink600, bar: default_.bar,
        accentText: default_.accentText, accentHover: default_.accentHover, accent: default_.accent,
        accentPress: default_.accentPress, accentRest: default_.accentRest, accentTint: default_.accentTint,
        ice300: Color(hex: 0xF1EADA), ice500: Color(hex: 0xCFC6B2), ice600: Color(hex: 0x9A9382), ice700: Color(hex: 0x6B6658),
        textPrimary: Color(hex: 0xE2DAC8), textSecondary: Color(hex: 0x9A9382),
        textTertiary: Color(hex: 0x6B6658), textDisabled: Color(hex: 0x4A4840),
        violet: default_.violet, coral300: default_.coral300, coral500: default_.coral500,
        ok: default_.ok, dangerTint: default_.dangerTint,
        groundTop: default_.groundTop, groundMid: default_.groundMid, groundBot: default_.groundBot
    )

    // Solarized's canonical palette (Ethan Schoonover):
    //   base03 002b36  base02 073642  base01 586e75  base00 657b83
    //   base0  839496  base1  93a2a2  base2  eee8d5  base3  fdf6e3
    //   yellow b58900  orange cb4b16  red    dc322f  magenta d33682
    //   violet 6c71c4  blue   268bd2  cyan   2aa198  green  859900

    /// Solarized Dark: base03 ground, blue accent.
    static let solarizedDark = Theme(
        id: "solarizedDark", name: "Solarized Dark",
        ink000: Color(hex: 0x00212B), ink050: Color(hex: 0x002B36), ink100: Color(hex: 0x073642),
        ink200: Color(hex: 0x08404E), ink300: Color(hex: 0x073642), ink400: Color(hex: 0x0B4A5A),
        ink500: Color(hex: 0x0E5265), ink600: Color(hex: 0x586E75), bar: Color(hex: 0x00252F),
        accentText: Color(hex: 0x6FC3E8), accentHover: Color(hex: 0x4BA9DA), accent: Color(hex: 0x268BD2),
        accentPress: Color(hex: 0x1C6FA8), accentRest: Color(hex: 0x175A87), accentTint: Color(hex: 0x08323F),
        ice300: Color(hex: 0xFDF6E3), ice500: Color(hex: 0xEEE8D5), ice600: Color(hex: 0x93A2A2), ice700: Color(hex: 0x586E75),
        textPrimary: Color(hex: 0x93A1A1), textSecondary: Color(hex: 0x839496),
        textTertiary: Color(hex: 0x657B83), textDisabled: Color(hex: 0x586E75),
        violet: Color(hex: 0x6C71C4), coral300: Color(hex: 0xE57368), coral500: Color(hex: 0xDC322F),
        ok: Color(hex: 0x859900), dangerTint: Color(hex: 0x3A1512),
        groundTop: Color(hex: 0x0A4453), groundMid: Color(hex: 0x002B36), groundBot: Color(hex: 0x001C24)
    )

    /// Solarized Light: base3 ground, blue accent, dark text.
    static let solarizedLight = Theme(
        id: "solarizedLight", name: "Solarized Light",
        ink000: Color(hex: 0xEEE8D5), ink050: Color(hex: 0xFDF6E3), ink100: Color(hex: 0xF3EDD8),
        ink200: Color(hex: 0xEFE8D0), ink300: Color(hex: 0xFBF4E0), ink400: Color(hex: 0xEAE3CC),
        ink500: Color(hex: 0xDCD5BE), ink600: Color(hex: 0x93A2A2), bar: Color(hex: 0xEEE8D5),
        accentText: Color(hex: 0x1C6FA8), accentHover: Color(hex: 0x2280BE), accent: Color(hex: 0x268BD2),
        accentPress: Color(hex: 0x1C6FA8), accentRest: Color(hex: 0x5A94BC), accentTint: Color(hex: 0xDCE9F2),
        ice300: Color(hex: 0x002B36), ice500: Color(hex: 0x073642), ice600: Color(hex: 0x586E75), ice700: Color(hex: 0x657B83),
        textPrimary: Color(hex: 0x073642), textSecondary: Color(hex: 0x586E75),
        textTertiary: Color(hex: 0x657B83), textDisabled: Color(hex: 0x93A2A2),
        violet: Color(hex: 0x6C71C4), coral300: Color(hex: 0xCB4B16), coral500: Color(hex: 0xDC322F),
        ok: Color(hex: 0x859900), dangerTint: Color(hex: 0xF3D9C8),
        groundTop: Color(hex: 0xFFFBF0), groundMid: Color(hex: 0xFDF6E3), groundBot: Color(hex: 0xF3EDD8)
    )
}

/// Observable wrapper so switching the theme redraws the app. Changing the
/// static `Tok.current` alone does not invalidate any SwiftUI view; bumping
/// this published value (and re-`.id()`-ing the root) does.
@MainActor
final class ThemeManager: ObservableObject {
    static let shared = ThemeManager()
    @Published private(set) var theme: Theme = Theme.load()

    func select(_ theme: Theme) {
        Theme.set(theme)
        self.theme = theme
    }

    /// The system colour scheme a theme wants, so text contrast is right.
    var colorScheme: ColorScheme {
        theme.id == "solarizedLight" ? .light : .dark
    }
}
