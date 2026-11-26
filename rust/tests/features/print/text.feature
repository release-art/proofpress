Feature: Text and line work
  Characters show real glyphs, fonts are embedded (Type 3 fonts are advisory), text and strokes meet the profile's
  floors, text stays inside the safe area, and text differs enough in lightness from its background.

  Scenario: Embedded text that meets every limit
    Given the fixture "embedded-text"
    When it is verified for print with '{"min_text_pt": 6.0, "min_stroke_pt": 0.25, "safe_mm": 3.0}'
    Then it passes

  Scenario: A character shown with .notdef
    Given the fixture "notdef-text"
    When it is verified for print
    Then check "missing-glyph" fails on page 1
    And check "font-embedded" fails on page 1

  Scenario: A Type 3 font
    Given the fixture "type3-text"
    When it is verified for print
    Then check "font-type3" fails on page 1

  Scenario: Text below the floor
    Given the fixture "small-text"
    When it is verified for print with '{"min_text_pt": 6.0}'
    Then check "text-size" fails on page 1

  Scenario: A hairline
    Given the fixture "hairline"
    When it is verified for print with '{"min_stroke_pt": 0.25}'
    Then check "hairline" fails on page 1

  Scenario: Text near the trim
    Given the fixture "edge-text"
    When it is verified for print with '{"safe_mm": 3.0}'
    Then check "safe-area" fails on page 1

  Scenario: Text too close to its background in lightness
    Given the fixture "low-contrast"
    When it is verified for print with '{"min_delta_l": 30}'
    Then check "contrast" fails on page 1

  Scenario: Text far from its background in lightness
    Given the fixture "high-contrast"
    When it is verified for print with '{"min_delta_l": 30}'
    Then no check fails but "font-embedded"
