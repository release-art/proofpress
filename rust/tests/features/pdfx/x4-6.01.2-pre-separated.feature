Feature: PDF/X-4 6.1.2, pre-separated files
  A file whose pages each paint a single colorant, different ones, is pre-separated (X4-Q2).

  Scenario: Two pages, Cyan and Magenta
    Given the fixture "x4-pre-separated"
    When it is verified as PDF/X-4
    Then check "x4.pre-separated.absent" fails
    And no check fails but "x4.pre-separated.absent"
