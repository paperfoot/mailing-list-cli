#!/bin/sh
# Minimal stub of email-cli for tests. Returns canned JSON for the
# subcommands mailing-list-cli actually calls.

# Strip the leading --json flag so the case statements work cleanly.
if [ "$1" = "--json" ]; then
    shift
fi

case "$1" in
    "agent-info")
        echo '{"name":"email-cli","version":"0.4.0","commands":{}}'
        exit 0
        ;;
    "profile")
        if [ "$2" = "test" ]; then
            echo '{"version":"1","status":"success","data":{"reachable":true}}'
            exit 0
        fi
        ;;
    "audience")
        case "$2" in
            "create")
                # Echo a fixed audience id back. Real email-cli emits an envelope.
                echo '{"version":"1","status":"success","data":{"id":"aud_test_12345","name":"stub"}}'
                exit 0
                ;;
            "list")
                echo '{"version":"1","status":"success","data":{"audiences":[]}}'
                exit 0
                ;;
        esac
        ;;
    "contact")
        case "$2" in
            "create")
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890"}}'
                exit 0
                ;;
            "list")
                echo '{"version":"1","status":"success","data":{"contacts":[]}}'
                exit 0
                ;;
        esac
        ;;
esac

echo '{"version":"1","status":"error","error":{"code":"unsupported","message":"stub","suggestion":"this is a test stub"}}' >&2
exit 1
