Feature: PDF/X-4 6.6, names
  Font and separation names, once their #xx escapes are decoded, are UTF-8, and every byte outside 33 to 126 is written
  as an escape.

  Scenario: A font name that is not UTF-8 fails
    Given the fixture "x4-name-not-utf8"
    When it is verified as PDF/X-4
    Then check "x4.names.utf8" fails
    And no check fails but "x4.names.utf8"

  Scenario: A font name with raw bytes outside 33 to 126 fails
    Given the fixture "x4-name-unescaped"
    When it is verified as PDF/X-4
    Then check "x4.names.hex-escaped" fails
    And no check fails but "x4.names.hex-escaped"
