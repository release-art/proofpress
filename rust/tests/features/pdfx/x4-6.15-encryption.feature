Feature: PDF/X-4 6.15, encryption and permissions
  The file is not encrypted, and the catalog's permissions hold only usage rights.

  Scenario: An encrypted file fails, even with an empty password
    Given the fixture "x4-encrypted"
    When it is verified as PDF/X-4
    Then check "x4.encrypt.absent" fails

  Scenario: A DocMDP permission fails
    Given the fixture "x4-perms"
    When it is verified as PDF/X-4
    Then check "x4.perms.ur-only" fails
    And no check fails but "x4.perms.ur-only"
