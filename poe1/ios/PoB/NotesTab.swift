import SwiftUI
import UIKit

/// PoB's Notes tab. The text is PoB's, colour codes and all: `^xRRGGBB`
/// sets a colour, `^0`–`^9` pick from SimpleGraphic's palette, and the
/// chips write the same codes PoB's buttons do, so a note written here
/// reads identically on the desktop.
struct NotesTab: View {
    @ObservedObject var model: BuildModel
    @State private var editing = false
    @State private var text = ""
    @State private var selection = NSRange(location: 0, length: 0)
    @State private var insertRequest: String?

    /// PoB's colour buttons, with PoB's own codes.
    static let chips: [(String, String)] = [
        ("NORMAL", "C8C8C8"), ("MAGIC", "8888FF"), ("RARE", "FFFF77"), ("UNIQUE", "AF6025"),
        ("FIRE", "B97123"), ("COLD", "3F6DB3"), ("LIGHTNING", "ADAA47"), ("CHAOS", "D02090"),
        ("STRENGTH", "E05030"), ("DEXTERITY", "70FF70"), ("INTELLIGENCE", "7070FF"),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: Tok.Space.s3) {
            HStack(spacing: Tok.Space.s3) {
                HStack(spacing: 2) {
                    modeButton("View", on: !editing) { commit(); editing = false }
                    modeButton("Edit", on: editing) { text = model.notes; editing = true }
                }
                .padding(2)
                .background(Tok.ink100)
                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
                Spacer()
            }

            if editing {
                VStack(alignment: .leading, spacing: Tok.Space.s2) {
                    SectionLabel(text: "Colour codes")
                    FlowRow(spacing: Tok.Space.s2) {
                        ForEach(Self.chips, id: \.0) { chip in
                            Button(chip.0) { insertRequest = "^x" + chip.1 }
                                .font(Tok.label).tracking(1)
                                .foregroundStyle(Color(hex: UInt32(chip.1, radix: 16) ?? 0xE8ECF4))
                                .padding(.horizontal, Tok.Space.s3).frame(height: 36)
                                .background(Tok.ink300)
                                .overlay(RoundedRectangle(cornerRadius: Tok.Radius.sm).stroke(Tok.ink500, lineWidth: 1))
                                .accessibilityLabel("Colour \(chip.0.capitalized)")
                        }
                    }
                    Text("Select text and tap a colour to wrap it, or tap to insert the code at the cursor. ^ followed by a hex code or 0–9 sets colour, like desktop PoB.")
                        .font(Tok.ui(13)).foregroundStyle(Tok.textTertiary).textLift()
                }
                NotesEditor(text: $text, selection: $selection, insert: $insertRequest, onEnd: { commit() })
                    .background(Tok.ink100)
                    .overlay(RoundedRectangle(cornerRadius: Tok.Radius.md).stroke(Tok.ink500, lineWidth: 1))
                    .clipShape(RoundedRectangle(cornerRadius: Tok.Radius.md))
            } else {
                ScrollView {
                    if model.notes.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        Text("No notes yet. Tap Edit to write some.").font(Tok.ui(15)).foregroundStyle(Tok.textTertiary).textLift()
                            .frame(maxWidth: .infinity, alignment: .leading).padding(Tok.Space.s4)
                    } else {
                        Text(Self.render(model.notes))
                            .font(Tok.mono(15))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(Tok.Space.s4)
                    }
                }
                .plate()
            }
        }
        .padding(Tok.Space.s4)
        .onDisappear { if editing { commit() } }
    }

    private func modeButton(_ label: String, on: Bool, _ act: @escaping () -> Void) -> some View {
        Button(label, action: act)
            .font(Tok.ui(15, .bold))
            .foregroundStyle(on ? Tok.ice500 : Tok.textSecondary).textLift()
            .frame(width: 72, height: 36)
            .background(on ? Tok.ink500 : .clear)
            .clipShape(RoundedRectangle(cornerRadius: 3))
            .buttonStyle(.plain)
            .accessibilityAddTraits(on ? [.isSelected] : [])
    }

    /// Write the note when leaving edit mode or the tab; not per keystroke.
    private func commit() {
        guard editing, text != model.notes else { return }
        model.setNotes(text)
        model.notes = text
        model.save()
    }

    /// PoB's colour codes to attributed text.
    static func render(_ s: String) -> AttributedString {
        let palette: [Character: UInt32] = ["0": 0x000000, "1": 0xFF0000, "2": 0x00FF00, "3": 0x0000FF, "4": 0xFFFF00,
                                           "5": 0xFF00FF, "6": 0x00FFFF, "7": 0xFFFFFF, "8": 0x808080, "9": 0x404040]
        var out = AttributedString()
        var colour = Color(hex: 0xE8ECF4)
        var i = s.startIndex
        var run = ""
        func flush() {
            if !run.isEmpty {
                var a = AttributedString(run); a.foregroundColor = colour; out += a; run = ""
            }
        }
        while i < s.endIndex {
            if s[i] == "^" {
                let n = s.index(after: i)
                if n < s.endIndex, s[n] == "x", let e = s.index(n, offsetBy: 7, limitedBy: s.endIndex),
                   let v = UInt32(s[s.index(after: n)..<e], radix: 16) {
                    flush(); colour = Color(hex: v); i = e; continue
                }
                if n < s.endIndex, let v = palette[s[n]] {
                    flush(); colour = v == 0xFFFFFF ? Tok.textPrimary : Color(hex: v); i = s.index(after: n); continue
                }
            }
            run.append(s[i]); i = s.index(after: i)
        }
        flush()
        return out
    }
}

/// A UITextView, because inserting at the cursor and wrapping a selection
/// need the selection, which SwiftUI's TextEditor does not expose.
struct NotesEditor: UIViewRepresentable {
    @Binding var text: String
    @Binding var selection: NSRange
    @Binding var insert: String?
    var onEnd: () -> Void

    func makeUIView(context: Context) -> UITextView {
        let v = UITextView()
        v.delegate = context.coordinator
        v.font = UIFont.monospacedSystemFont(ofSize: 15, weight: .regular)
        v.textColor = UIColor(Tok.textPrimary)
        v.backgroundColor = .clear
        v.autocorrectionType = .no
        v.autocapitalizationType = .none
        v.smartQuotesType = .no
        v.smartDashesType = .no
        v.textContainerInset = UIEdgeInsets(top: 12, left: 10, bottom: 12, right: 10)
        v.text = text
        v.accessibilityLabel = "Notes"
        return v
    }

    func updateUIView(_ v: UITextView, context: Context) {
        if v.text != text, !context.coordinator.typing { v.text = text }
        if let code = insert {
            // PoB's SetColor: wrap a selection in the colour and reset to
            // white after it; with nothing selected, insert the code.
            let r = v.selectedRange
            let ns = v.text as NSString
            let sel = ns.substring(with: r)
            let piece = sel.isEmpty ? code : code + sel + "^7"
            v.textStorage.replaceCharacters(in: r, with: piece)
            v.selectedRange = NSRange(location: r.location + piece.count, length: 0)
            DispatchQueue.main.async {
                text = v.text
                insert = nil
            }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, UITextViewDelegate {
        let parent: NotesEditor
        var typing = false
        init(_ p: NotesEditor) { parent = p }
        func textViewDidChange(_ v: UITextView) {
            typing = true
            parent.text = v.text
            typing = false
        }
        func textViewDidChangeSelection(_ v: UITextView) { parent.selection = v.selectedRange }
        func textViewDidEndEditing(_ v: UITextView) { parent.onEnd() }
    }
}
