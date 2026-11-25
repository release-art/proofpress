Feature: PDF/X-4 6.25, architectural limits
  The file stays within Table C.1 of the PDF 1.6 Reference.

  Scenario: One indirect object too many fails
    Given the fixture "x4-too-many-objects"
    When it is verified as PDF/X-4
    Then check "x4.limits.objects" fails
    And no check fails but "x4.limits.objects"
