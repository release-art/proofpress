Feature: The finished page and the job's shape
  Pages are the expected trim size, with the expected bleed, centred on the media box; the page count fits the rule,
  pages share one size and are not rotated; the output intent names the expected condition; artwork runs into the
  bleed and nothing lies beyond it.

  Scenario: A good A6 card meets an A6 profile
    Given the fixture "good"
    When it is verified for print with '{"geometry": {"trim_w_mm": 105.0, "trim_h_mm": 148.0, "bleed_mm": 3.0}, "uniform_pages": true, "output_condition": "sRGB"}'
    Then it passes

  Scenario: An A5 page where A6 is wanted
    Given the fixture "wrong-size"
    When it is verified for print with '{"geometry": {"trim_w_mm": 105.0, "trim_h_mm": 148.0, "bleed_mm": 3.0}}'
    Then check "geometry" fails on page 1
    And check "bleed" fails on page 1

  Scenario: A book interior with no bleed on the binding edge, against a profile that wants it all round
    Given the fixture "book-interior"
    When it is verified for print with '{"geometry": {"trim_w_mm": 105.0, "trim_h_mm": 148.0, "bleed_mm": 3.0}}'
    Then check "bleed" fails on page 1
    And check "centring" fails on page 2

  Scenario: Pages of two sizes, and a count that is not a multiple
    Given the fixture "two-sizes"
    When it is verified for print with '{"uniform_pages": true, "pages_multiple_of": 4}'
    Then check "page-size" fails on page 2
    And check "page-count" fails

  Scenario: The output intent names another condition
    Given the fixture "crop-marks"
    When it is verified for print with '{"output_condition": "FOGRA39"}'
    Then check "output-condition" fails
    And check "outside-bleed" fails on page 1

  Scenario: Artwork stops at the trim
    Given the fixture "bleed-short"
    When it is verified for print
    Then check "artwork-bleed" fails on page 1
