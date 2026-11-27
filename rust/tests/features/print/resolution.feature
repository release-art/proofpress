Feature: Image resolution
  Images are placed at the profile's minimum resolution or finer, not so fine that they only grow the file, and not
  scaled much more on one axis than the other.

  Scenario: An image at a good resolution
    Given the fixture "image-resolution-ok"
    When it is verified for print with '{"max_dpi": 1200.0, "max_stretch_pct": 5.0}'
    Then it passes

  Scenario: An image below the minimum
    Given the fixture "low-dpi-image"
    When it is verified for print
    Then check "image-dpi" fails on page 1

  Scenario: An image far finer than needed
    Given the fixture "image-too-sharp"
    When it is verified for print with '{"max_dpi": 600.0}'
    Then check "image-dpi-max" fails on page 1

  Scenario: An image stretched on one axis
    Given the fixture "image-stretched"
    When it is verified for print with '{"max_stretch_pct": 5.0}'
    Then check "image-stretch" fails on page 1
