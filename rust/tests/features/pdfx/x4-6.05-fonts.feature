Feature: PDF/X-4 6.5, fonts
  Fonts that text shows visibly are embedded with a glyph for each code (6.5.1), no code shows .notdef (6.5.2), widths
  agree with the program (6.5.3), and TrueType fonts follow 6.5.4's encoding rules: a non-symbolic font names MacRomanEncoding or
  WinAnsiEncoding without /Differences; a symbolic one has no /Encoding and one cmap subtable.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture                  | check                                       |
      | x4-font-not-embedded     | x4.font.embedded                            |
      | x4-font-hidden-oc        | x4.font.embedded                            |
      | x4-font-glyph-missing    | x4.font.glyphs-present                      |
      | x4-font-notdef           | x4.font.no-notdef                           |
      | x4-font-widths           | x4.font.widths-consistent                   |
      | x4-tt-no-encoding        | x4.font.truetype-nonsymbolic-encoding       |
      | x4-tt-differences        | x4.font.truetype-nonsymbolic-no-differences |
      | x4-tt-symbolic-encoding  | x4.font.truetype-symbolic-no-encoding       |
      | x4-tt-symbolic-two-cmaps | x4.font.truetype-symbolic-one-cmap          |

  Scenario: An unembedded font used only in render mode 3 passes
    Given the fixture "x4-font-invisible-unembedded"
    When it is verified as PDF/X-4
    Then it passes

  Scenario: A code with no glyph shown only in render mode 3 passes
    Given the fixture "x4-font-invisible-missing"
    When it is verified as PDF/X-4
    Then it passes
