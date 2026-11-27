Feature: What a page paints, and where
  A Do that names no XObject leaves a hole where the artwork should be (ISO 32000-2 7.8.3), and an object painted
  wholly beyond the MediaBox never prints but ships in the file (a warning); it lies beyond the bleed too.

  Scenario: The good file paints nothing amiss
    Given the fixture "x4-good"
    When it is verified for print
    Then it passes

  Scenario: A Do with nothing behind it fails
    Given the fixture "print-xobject-missing"
    When it is verified for print
    Then check "xobject-missing" fails on page 1
    And no check fails but "xobject-missing"

  Scenario: An object wholly beyond the MediaBox is a warning
    Given the fixture "print-outside-media"
    When it is verified for print
    Then check "outside-media" fails on page 1
    And check "outside-bleed" fails on page 1
