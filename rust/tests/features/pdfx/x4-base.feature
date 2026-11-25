Feature: The genuine PDF/X-4 base fixture
  x4-good is a real PDF/X-4 file: every PDF/X negative fixture is it with one thing changed, so it must pass every
  PDF/X-4 check. pdfToolbox 17.1's "Verify compliance with PDFX-4" passes it too.

  Scenario: x4-good passes as PDF/X-4
    Given the fixture "x4-good"
    When it is verified as PDF/X-4
    Then it passes
