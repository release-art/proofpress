Feature: ISO 32000-2 7.7, the document structure
  Every page has a MediaBox, its own or inherited from the page tree (7.7.3.3).

  Scenario: A page without a MediaBox raises its check and, at PDF/X-4, 6.1.1's
    Given the fixture "x4-no-media-box"
    When it is verified as PDF/X-4
    Then check "page.media-box" fails
    And check "x4.pdf-reference.conforms" fails
    And no check fails but "page.media-box" and "x4.pdf-reference.conforms"
