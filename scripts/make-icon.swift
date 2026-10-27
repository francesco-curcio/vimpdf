// Draws the pdfv app icon (1024×1024 PNG). Used by scripts/make-icon.sh.
// Usage: swift scripts/make-icon.swift out.png
import AppKit

let size = 1024.0
let out = CommandLine.arguments[1]
let image = NSImage(size: NSSize(width: size, height: size))
image.lockFocus()

// Background: dark rounded square (macOS icon grid: 824 px body, 100 px inset).
let body = NSRect(x: 100, y: 100, width: 824, height: 824)
NSColor(srgbRed: 0.12, green: 0.12, blue: 0.18, alpha: 1).setFill()
NSBezierPath(roundedRect: body, xRadius: 185, yRadius: 185).fill()

// A page with a folded corner.
let page = NSBezierPath()
page.move(to: NSPoint(x: 300, y: 230))
page.line(to: NSPoint(x: 724, y: 230))
page.line(to: NSPoint(x: 724, y: 700))
page.line(to: NSPoint(x: 614, y: 810))
page.line(to: NSPoint(x: 300, y: 810))
page.close()
NSColor(srgbRed: 0.85, green: 0.86, blue: 0.9, alpha: 1).setFill()
page.fill()
let fold = NSBezierPath()
fold.move(to: NSPoint(x: 614, y: 810))
fold.line(to: NSPoint(x: 614, y: 700))
fold.line(to: NSPoint(x: 724, y: 700))
fold.close()
NSColor(srgbRed: 0.65, green: 0.67, blue: 0.75, alpha: 1).setFill()
fold.fill()

// Text lines, one highlighted like a visual-mode selection, and a caret.
NSColor(srgbRed: 0.35, green: 0.37, blue: 0.47, alpha: 1).setFill()
for (i, w) in [300.0, 340.0, 260.0, 320.0].enumerated() {
    NSBezierPath(roundedRect: NSRect(x: 350, y: 620 - Double(i) * 70, width: w, height: 26),
                 xRadius: 13, yRadius: 13).fill()
}
NSColor(srgbRed: 0.54, green: 0.71, blue: 0.98, alpha: 0.55).setFill()
NSBezierPath(roundedRect: NSRect(x: 340, y: 538, width: 300, height: 48), xRadius: 10, yRadius: 10).fill()
NSColor(srgbRed: 0.95, green: 0.55, blue: 0.66, alpha: 1).setFill()
NSRect(x: 636, y: 530, width: 12, height: 64).fill()

image.unlockFocus()
let rep = NSBitmapImageRep(data: image.tiffRepresentation!)!
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
